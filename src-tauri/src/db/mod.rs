//! 관찰 기록 저장소: SQLite(rusqlite)와 첨부 파일 폴더.
//!
//! DB 로직은 `&Connection`을 받는 순수 함수로 두고, `#[tauri::command]`는 얇게 감싼다.

pub mod activities;
pub mod attachments;
pub mod classes;
pub mod crypt;
pub mod evaluation;
pub mod habits;
pub mod key;
pub mod migrations;
pub mod observations;
pub mod plays;
pub mod portfolio;
pub mod reports;
pub mod settings;
pub mod students;
pub mod testing;
pub mod units;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

/// 모든 DB 명령의 오류는 화면에 보여 줄 한국어 문자열이다.
pub type DbResult<T> = Result<T, String>;

/// 첨부 파일 폴더(`.../data/media`). 관리 상태로 둔다.
pub struct MediaDir(pub PathBuf);

/// rusqlite 오류를 한국어 문자열로 바꾸는 확장.
pub trait DbExt<T> {
    fn db(self) -> DbResult<T>;
}

impl<T> DbExt<T> for Result<T, rusqlite::Error> {
    fn db(self) -> DbResult<T> {
        self.map_err(|e| format!("데이터베이스 오류: {e}"))
    }
}

/// 현재 로컬 시각(ISO 8601, 초 단위, 오프셋 포함).
pub fn now_iso() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 연결에 공통 설정(외래 키 켜기)을 하고 마이그레이션을 적용한다.
pub fn prepare(conn: &Connection) -> DbResult<()> {
    conn.pragma_update(None, "foreign_keys", "ON").db()?;
    migrations::apply(conn)
}

/// 기록 DB를 열었을 때의 상태. 설계: docs/design/20-db-encryption.md 3절.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbState {
    /// 암호화되어 있고 잘 열렸다.
    Ok,
    /// 평문으로 계속 쓰는 중(전환을 하지 못했다). 이유는 화면에 보여 줄 문장.
    Plain(String),
    /// 암호화된 기록이 있으나 이 계정에서 열 수 없다. 빈 메모리 DB로 앱을 띄우고 안내 화면만 보인다.
    Locked(String),
}

impl DbState {
    pub fn kind(&self) -> &'static str {
        match self {
            DbState::Ok => "ok",
            DbState::Plain(_) => "plain",
            DbState::Locked(_) => "locked",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            DbState::Ok => None,
            DbState::Plain(r) | DbState::Locked(r) => Some(r),
        }
    }

    /// 기록 파일이 암호화되어 있는지(잠긴 상태도 암호화된 파일이 있는 것이다).
    pub fn encrypted(&self) -> bool {
        !matches!(self, DbState::Plain(_))
    }
}

/// 화면에 알리는 DB 상태.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbStateInfo {
    /// `ok` | `plain` | `locked`
    pub state: &'static str,
    pub reason: Option<String>,
}

impl From<&DbState> for DbStateInfo {
    fn from(s: &DbState) -> Self {
        Self { state: s.kind(), reason: s.reason().map(str::to_string) }
    }
}

/// 관리 상태로 두는 현재 DB 상태. 복원에 성공하면 `Ok`로 바뀐다.
pub struct DbStatus(pub Mutex<DbState>);

pub fn status_of(m: &Mutex<DbState>) -> DbState {
    m.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
pub fn db_state(state: tauri::State<DbStatus>) -> DbStateInfo {
    (&status_of(&state.0)).into()
}

pub struct Opened {
    pub conn: Connection,
    pub media: MediaDir,
    pub state: DbState,
}

fn finish_plain(path: &Path) -> DbResult<Connection> {
    finish_conn(Connection::open(path).db()?)
}

/// 페이지 캐시 크기(KiB, 음수로 지정). 암호화 DB는 캐시에 없는 페이지를 읽을 때마다 복호화·검증을 하므로,
/// 기본(2MB)보다 넉넉하게 잡아 1년치 DB(약 10MB)의 페이지가 풀린 채 캐시에 머물게 한다. 쓴 만큼만 메모리를 쓴다.
const CACHE_KIB: i64 = 32 * 1024;

fn finish_conn(conn: Connection) -> DbResult<Connection> {
    conn.pragma_update(None, "journal_mode", "WAL").db()?;
    conn.pragma_update(None, "cache_size", -CACHE_KIB).db()?;
    prepare(&conn)?;
    Ok(conn)
}

/// 잠긴 상태로 앱을 띄울 때 쓰는 빈 메모리 DB. 명령이 "표 없음" 오류를 내지 않도록 스키마만 만든다.
fn placeholder() -> DbResult<Connection> {
    let conn = Connection::open_in_memory().db()?;
    prepare(&conn)?;
    Ok(conn)
}

/// `data/tonghap.db`를 연다. 폴더가 없으면 만든다. 설계 3절의 상태표를 따른다.
/// 암호화된 DB를 열 수 없으면 **새 DB를 만들지 않고** `DbState::Locked`로 돌려준다.
pub fn open_full(data_dir: &Path) -> DbResult<Opened> {
    open_full_with(data_dir, &crypt::RealOps)
}

pub fn open_full_with(data_dir: &Path, ops: &dyn crypt::ConvertOps) -> DbResult<Opened> {
    use key::KeyFile;
    let media_dir = data_dir.join("media");
    std::fs::create_dir_all(&media_dir).map_err(|e| format!("데이터 폴더를 만들 수 없습니다: {e}"))?;
    let media = MediaDir(media_dir);
    let db_path = data_dir.join(crypt::DB_NAME);

    let key_file = key::read_key_file(data_dir);
    let usable = match &key_file {
        KeyFile::Key(k) => Some(k),
        _ => None,
    };
    crypt::recover_leftovers(data_dir, usable);

    let locked = |why: String| -> DbResult<Opened> {
        crate::logging::error("db", &format!("기록을 열 수 없습니다: {why}"));
        Ok(Opened { conn: placeholder()?, media: MediaDir(data_dir.join("media")), state: DbState::Locked(why) })
    };

    let kind = crypt::file_kind(&db_path)?;
    if kind == crypt::FileKind::Encrypted {
        return match key_file {
            KeyFile::Key(k) => match crypt::open_keyed(&db_path, &k) {
                Ok(conn) => {
                    let conn = finish_conn(conn)?;
                    convert_old_folders(data_dir, &k, ops);
                    Ok(Opened { conn, media, state: DbState::Ok })
                }
                Err(crypt::KeyedOpenError::WrongKey) => locked("저장된 키로 기록 파일을 열 수 없습니다".to_string()),
                Err(crypt::KeyedOpenError::Other(m)) => Err(m),
            },
            KeyFile::Missing => locked("암호화 키 파일이 없습니다".to_string()),
            KeyFile::Unreadable(why) => locked(why),
        };
    }

    // 없거나 평문: 키를 준비한다. 풀 수 없는 키 파일은 지우지 않고 옆으로 치워 둔다.
    let key = match key_file {
        KeyFile::Key(k) => k,
        other => {
            if matches!(other, KeyFile::Unreadable(_)) {
                key::set_aside_key_file(data_dir);
            }
            let k = key::DbKey::generate()?;
            key::write_key_file(data_dir, &k)?;
            k
        }
    };
    let mut state = DbState::Ok;
    if kind == crypt::FileKind::Plain {
        if let crypt::Converted::Skipped(why) = crypt::convert_plain_db(&db_path, &key, ops) {
            state = DbState::Plain(why);
        }
    }
    let conn = if state == DbState::Ok {
        finish_conn(crypt::open_keyed(&db_path, &key).map_err(|e| e.message())?)?
    } else {
        finish_plain(&db_path)?
    };
    convert_old_folders(data_dir, &key, ops);
    Ok(Opened { conn, media, state })
}

/// 예전 복원 보관 폴더(`before-restore-*`)의 평문 DB도 암호화한다. 실패해도 앱 시작을 막지 않는다.
fn convert_old_folders(data_dir: &Path, key: &key::DbKey, ops: &dyn crypt::ConvertOps) {
    let Ok(rd) = std::fs::read_dir(data_dir) else { return };
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().starts_with("before-restore-") || !e.path().is_dir() {
            continue;
        }
        let dir = e.path();
        crypt::recover_leftovers(&dir, Some(key));
        let db = dir.join(crypt::DB_NAME);
        if crypt::file_kind(&db) == Ok(crypt::FileKind::Plain) {
            let _ = crypt::convert_plain_db(&db, key, ops);
        }
    }
}

/// `open_full`을 감싼 간단한 열기(테스트·복원 교체용). 잠긴 상태는 오류로 돌려준다.
pub fn open(data_dir: &Path) -> DbResult<(Connection, MediaDir)> {
    let o = open_full(data_dir)?;
    if let DbState::Locked(why) = &o.state {
        return Err(format!("기록을 열 수 없습니다: {why}"));
    }
    Ok((o.conn, o.media))
}

pub fn lock(m: &Mutex<Connection>) -> DbResult<MutexGuard<'_, Connection>> {
    m.lock().map_err(|_| "데이터베이스가 잠겨 있습니다. 앱을 다시 시작해 주세요".to_string())
}

#[cfg(test)]
pub fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    prepare(&conn).unwrap();
    conn
}
