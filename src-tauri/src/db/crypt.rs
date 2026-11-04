//! SQLCipher 파일 다루기: 키로 열기, 평문 ↔ 암호화 내보내기, 전환(평문 → 암호화), 남은 파일 복구.
//! 설계: docs/design/20-db-encryption.md 3~4절.
//!
//! 전환의 원칙: 새 암호화 DB가 원본과 같다는 것이 확인되기 전에는 원본 평문 DB를 건드리지 않고,
//! 바꾸기 도중 어떤 단계가 실패해도 평문 DB를 원래 이름으로 되돌린다. 평문을 지우는 것은 맨 마지막이다.
//! 로그에는 경로·키를 남기지 않는다.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode};

use super::key::DbKey;
use super::{DbExt, DbResult};
use crate::logging;

pub const DB_NAME: &str = "tonghap.db";
pub const ENC_TMP: &str = "tonghap.db.enc-tmp";
pub const PLAIN_OLD: &str = "tonghap.db.plain-old";
const SUFFIXES: [&str; 3] = ["", "-wal", "-shm"];
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// `path` 뒤에 글자를 붙인 경로(`tonghap.db` + `-wal`).
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// DB 파일과 -wal, -shm, -journal을 지운다(없으면 넘어간다).
pub fn remove_db_files(path: &Path) {
    for s in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(with_suffix(path, s));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// 없거나 0바이트
    Missing,
    /// 첫 16바이트가 `SQLite format 3\0`
    Plain,
    /// 그 밖(암호화 DB 또는 알 수 없는 내용). 함부로 지우거나 덮어쓰지 않는다.
    Encrypted,
}

pub fn file_kind(path: &Path) -> DbResult<FileKind> {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileKind::Missing),
        Err(e) => return Err(format!("데이터베이스 파일을 읽을 수 없습니다: {e}")),
    };
    let mut head = [0u8; 16];
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) => return Err(format!("데이터베이스 파일을 읽을 수 없습니다: {e}")),
        }
    }
    Ok(match n {
        0 => FileKind::Missing,
        16 if &head == SQLITE_MAGIC => FileKind::Plain,
        _ => FileKind::Encrypted,
    })
}

fn quote_path(p: &Path) -> String {
    p.to_string_lossy().replace('\'', "''")
}

// ---------- 열기 ----------

#[derive(Debug)]
pub enum KeyedOpenError {
    /// 키가 맞지 않거나 암호화 DB가 아님
    WrongKey,
    Other(String),
}

impl KeyedOpenError {
    pub fn message(&self) -> String {
        match self {
            KeyedOpenError::WrongKey => "기록 파일을 이 키로 열 수 없습니다".to_string(),
            KeyedOpenError::Other(m) => m.clone(),
        }
    }
}

/// 키를 넣고 `sqlite_master`를 읽어 키가 맞는지 확인한다. 파일이 없으면 새로 만든다(호출 쪽이 조건을 확인한다).
pub fn open_keyed(path: &Path, key: &DbKey) -> Result<Connection, KeyedOpenError> {
    let conn = Connection::open(path).map_err(|e| KeyedOpenError::Other(format!("데이터베이스를 열 수 없습니다: {e}")))?;
    conn.execute_batch(&format!("PRAGMA key = \"{}\";", key.sql_literal()))
        .map_err(|_| KeyedOpenError::Other("암호화 키를 설정하지 못했습니다".to_string()))?;
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)) {
        Ok(_) => Ok(conn),
        Err(e) if e.sqlite_error_code() == Some(ErrorCode::NotADatabase) => Err(KeyedOpenError::WrongKey),
        Err(e) if e.sqlite_error_code() == Some(ErrorCode::DatabaseCorrupt) => Err(KeyedOpenError::WrongKey),
        Err(e) => Err(KeyedOpenError::Other(format!("데이터베이스를 읽을 수 없습니다: {e}"))),
    }
}

// ---------- 비교 ----------

fn table_counts(conn: &Connection) -> DbResult<Vec<(String, i64)>> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").db()?;
    let names: Vec<String> = stmt.query_map([], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    names
        .into_iter()
        .map(|n| {
            let c: i64 = conn.query_row(&format!("SELECT count(*) FROM \"{}\"", n.replace('"', "\"\"")), [], |r| r.get(0)).db()?;
            Ok((n, c))
        })
        .collect()
}

fn integrity_ok(conn: &Connection) -> bool {
    conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).is_ok_and(|s| s == "ok")
}

fn user_version(conn: &Connection) -> DbResult<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0)).db()
}

fn schema_objects(conn: &Connection) -> DbResult<i64> {
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0)).db()
}

/// 두 연결의 내용이 같은지(스키마 버전, 스키마 개수, 모든 사용자 표의 행 수) 확인하고 `b`의 무결성을 검사한다.
pub fn verify_same(a: &Connection, b: &Connection) -> DbResult<()> {
    if !integrity_ok(b) {
        return Err("새 파일의 무결성 검사가 통과하지 못했습니다".to_string());
    }
    if user_version(a)? != user_version(b)? {
        return Err("스키마 버전이 서로 다릅니다".to_string());
    }
    if schema_objects(a)? != schema_objects(b)? {
        return Err("표 구성이 서로 다릅니다".to_string());
    }
    if table_counts(a)? != table_counts(b)? {
        return Err("행 수가 서로 다릅니다".to_string());
    }
    Ok(())
}

/// 평문 DB 파일과 암호화 DB 파일이 같은 내용인지 확인한다(둘 다 열었다가 닫는다).
pub fn verify_pair(plain: &Path, enc: &Path, key: &DbKey) -> DbResult<()> {
    let p = Connection::open(plain).db()?;
    let e = open_keyed(enc, key).map_err(|e| e.message())?;
    verify_same(&p, &e)
}

// ---------- 내보내기 ----------

/// 평문 DB `plain`의 내용을 키로 암호화한 새 파일 `dest`로 내보낸다.
pub fn export_to_encrypted(plain: &Path, dest: &Path, key: &DbKey) -> DbResult<()> {
    let conn = Connection::open(plain).db()?;
    let version = user_version(&conn)?;
    conn.execute_batch(&format!(
        "ATTACH DATABASE '{}' AS enc KEY \"{}\"; SELECT sqlcipher_export('enc'); PRAGMA enc.user_version = {version}; DETACH DATABASE enc;",
        quote_path(dest),
        key.sql_literal()
    ))
    .map_err(|e| format!("암호화 파일을 만들지 못했습니다: {e}"))
}

/// 열려 있는(키가 걸린 또는 평문) 연결의 내용을 평문 SQLite 파일 `dest`로 내보낸다. 백업 보관함용.
pub fn export_to_plain(conn: &Connection, dest: &Path) -> DbResult<()> {
    let version = user_version(conn)?;
    let r = conn.execute_batch(&format!(
        "ATTACH DATABASE '{}' AS plaintext KEY ''; SELECT sqlcipher_export('plaintext'); PRAGMA plaintext.user_version = {version};",
        quote_path(dest)
    ));
    // 실패했어도 붙여 둔 채로 두지 않는다(앱의 연결은 계속 쓰인다).
    let _ = conn.execute_batch("DETACH DATABASE plaintext");
    r.map_err(|e| format!("백업용 복사본을 만들지 못했습니다: {e}"))
}

/// 평문 DB 파일 `db`를 같은 자리에서 암호화 DB로 바꾼다(복원 임시 폴더용). 실패하면 `db`는 그대로다.
pub fn encrypt_file_in_place(db: &Path, key: &DbKey) -> DbResult<()> {
    let tmp = with_suffix(db, ".enc");
    let bak = with_suffix(db, ".plain-bak");
    remove_db_files(&tmp);
    remove_db_files(&bak);
    let r = (|| {
        export_to_encrypted(db, &tmp, key)?;
        verify_pair(db, &tmp, key)?;
        // 원본을 지우기 전에 옆으로 옮겨 두고, 바꾸기에 실패하면 되돌린다(어느 순간에도 사본 하나는 남는다).
        let _ = std::fs::remove_file(with_suffix(db, "-wal"));
        let _ = std::fs::remove_file(with_suffix(db, "-shm"));
        std::fs::rename(db, &bak).map_err(|e| format!("원본 파일을 옮기지 못했습니다: {e}"))?;
        if let Err(e) = std::fs::rename(&tmp, db) {
            let _ = std::fs::rename(&bak, db);
            return Err(format!("암호화한 파일로 바꾸지 못했습니다: {e}"));
        }
        remove_db_files(&bak);
        Ok(())
    })();
    if r.is_err() {
        remove_db_files(&tmp);
    }
    r
}

// ---------- 전환 ----------

/// 전환에서 바깥 세계와 닿는 동작. 테스트가 실패를 끼워 넣을 수 있도록 나누어 두었다.
pub trait ConvertOps {
    fn export(&self, plain: &Path, dest: &Path, key: &DbKey) -> DbResult<()> {
        export_to_encrypted(plain, dest, key)
    }
    fn rename(&self, from: &Path, to: &Path) -> DbResult<()> {
        std::fs::rename(from, to).map_err(|e| format!("파일 이름을 바꾸지 못했습니다: {e}"))
    }
    /// `dir`이 있는 디스크의 여유 공간(알 수 없으면 `None`)
    fn free_space(&self, dir: &Path) -> Option<u64> {
        disk_free(dir)
    }
}

pub struct RealOps;
impl ConvertOps for RealOps {}

fn disk_free(dir: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list().ok()?;
    disks
        .iter()
        .filter(|d| dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Converted {
    Done,
    /// 전환하지 않고 평문 그대로 둔다(이유는 화면에 보여 줄 한국어 문장).
    Skipped(String),
}

fn skip(why: impl Into<String>) -> Converted {
    let why = why.into();
    logging::error("db", &format!("기록 암호화 전환을 하지 않았습니다: {why}"));
    Converted::Skipped(why)
}

fn db_bytes(db: &Path) -> u64 {
    SUFFIXES.iter().map(|s| std::fs::metadata(with_suffix(db, s)).map(|m| m.len()).unwrap_or(0)).sum()
}

/// 평문 `db`(예: `.../tonghap.db`)를 암호화 DB로 바꾼다. 설계 4절의 1~6단계.
pub fn convert_plain_db(db: &Path, key: &DbKey, ops: &dyn ConvertOps) -> Converted {
    let dir = db.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(ENC_TMP);
    let old = dir.join(PLAIN_OLD);
    logging::info("db", "기록 암호화 전환 시작");

    // 이전 전환에서 남은 평문 보관본이 있으면 건드리지 않는다(그것이 유일한 사본일 수 있다).
    if old.exists() {
        return skip("이전 전환에서 남은 파일이 있어 전환하지 않았습니다");
    }

    // 1) 체크포인트와 무결성 검사
    {
        let conn = match Connection::open(db) {
            Ok(c) => c,
            Err(_) => return skip("기록 파일을 열 수 없어 암호화하지 못했습니다"),
        };
        let cp = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0));
        if !cp.is_ok_and(|busy| busy == 0) {
            return skip("기록 파일을 정리하지 못해 암호화하지 못했습니다");
        }
        if !integrity_ok(&conn) {
            return skip("기록 파일 검사에서 문제가 발견되어 암호화하지 않았습니다. 먼저 백업을 만들어 두세요");
        }
    }

    // 2) 디스크 여유
    let need = db_bytes(db).saturating_mul(2);
    if let Some(free) = ops.free_space(dir) {
        if free < need {
            return skip("디스크 여유 공간이 부족해 암호화를 미뤘습니다. 공간을 확보한 뒤 앱을 다시 시작해 주세요");
        }
    }

    // 3) 임시 암호화 파일 만들기
    remove_db_files(&tmp);
    if let Err(e) = ops.export(db, &tmp, key) {
        remove_db_files(&tmp);
        return skip(format!("암호화 파일을 만들지 못했습니다({e})"));
    }

    // 4) 검증: 무결성, 스키마, 모든 표의 행 수
    if let Err(e) = verify_pair(db, &tmp, key) {
        remove_db_files(&tmp);
        return skip(format!("암호화한 파일이 원본과 같지 않아 쓰지 않았습니다({e})"));
    }

    // 5) 바꾸기: 평문 → plain-old, tmp → tonghap.db
    let mut moved: Vec<&str> = Vec::new();
    let mut placed = false;
    let swapped = (|| -> DbResult<()> {
        for s in SUFFIXES {
            let from = with_suffix(db, s);
            if from.exists() {
                ops.rename(&from, &with_suffix(&old, s))?;
                moved.push(s);
            }
        }
        ops.rename(&tmp, db)?;
        placed = true;
        // 새 파일을 키로 다시 열어 원본(plain-old)과 같은지 확인한다.
        verify_pair(&old, db, key)
    })();
    if let Err(e) = swapped {
        // 우리가 놓은 새 파일만 지우고 평문을 원래 이름으로 되돌린다.
        if placed {
            remove_db_files(db);
        }
        for s in moved.iter().rev() {
            let _ = std::fs::rename(with_suffix(&old, s), with_suffix(db, s));
        }
        remove_db_files(&tmp);
        return skip(format!("파일을 바꾸는 중 문제가 생겨 원래대로 되돌렸습니다({e})"));
    }

    // 6) 평문 보관본 지우기(맨 마지막)
    remove_db_files(&old);
    if old.exists() {
        logging::error("db", "평문 보관본을 지우지 못했습니다. 다음 시작에서 다시 지웁니다");
    }
    logging::info("db", "기록 암호화 전환 완료");
    Converted::Done
}

// ---------- 시작할 때 남은 파일 정리 ----------

/// 전환 도중 꺼진 흔적(`enc-tmp`, `plain-old`)을 정리한다. 설계 4절 7단계.
pub fn recover_leftovers(data_dir: &Path, key: Option<&DbKey>) {
    let db = data_dir.join(DB_NAME);
    let tmp = data_dir.join(ENC_TMP);
    let old = data_dir.join(PLAIN_OLD);
    remove_db_files(&tmp);
    if file_kind(&old) != Ok(FileKind::Plain) {
        return;
    }
    match file_kind(&db) {
        Ok(FileKind::Missing) => {
            // 새 파일을 놓기 전에 꺼졌다: 평문을 되돌려 놓는다(다음 전환이 처음부터 다시 한다).
            for s in SUFFIXES {
                let from = with_suffix(&old, s);
                if from.exists() {
                    let _ = std::fs::rename(&from, with_suffix(&db, s));
                }
            }
            logging::info("db", "중단된 암호화 전환을 되돌렸습니다");
        }
        Ok(FileKind::Encrypted) => {
            // 새 파일이 이미 놓였다: 키로 열려 원본과 같을 때만 평문 보관본을 지운다.
            let Some(key) = key else { return };
            match verify_pair(&old, &db, key) {
                Ok(()) => {
                    remove_db_files(&old);
                    logging::info("db", "중단된 암호화 전환을 마무리했습니다");
                }
                Err(e) => logging::error("db", &format!("평문 보관본을 남겨 둡니다: {e}")),
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
