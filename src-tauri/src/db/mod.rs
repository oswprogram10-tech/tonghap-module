//! 관찰 기록 저장소: SQLite(rusqlite)와 첨부 파일 폴더.
//!
//! DB 로직은 `&Connection`을 받는 순수 함수로 두고, `#[tauri::command]`는 얇게 감싼다.

pub mod attachments;
pub mod classes;
pub mod migrations;
pub mod observations;
pub mod portfolio;
pub mod settings;
pub mod students;
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

/// `data/tonghap.db`를 연다. 폴더가 없으면 만든다.
pub fn open(data_dir: &Path) -> DbResult<(Connection, MediaDir)> {
    let media = data_dir.join("media");
    std::fs::create_dir_all(&media).map_err(|e| format!("데이터 폴더를 만들 수 없습니다: {e}"))?;
    let conn = Connection::open(data_dir.join("tonghap.db")).db()?;
    conn.pragma_update(None, "journal_mode", "WAL").db()?;
    prepare(&conn)?;
    Ok((conn, MediaDir(media)))
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
