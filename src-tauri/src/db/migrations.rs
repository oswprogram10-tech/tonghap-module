//! `PRAGMA user_version`으로 버전을 관리하는 순차 마이그레이션.

use rusqlite::Connection;

use super::{DbExt, DbResult};

/// 인덱스 0이 버전 1로 가는 SQL. 이미 배포된 항목은 고치지 말고 뒤에 추가한다.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE classes (
  id TEXT PRIMARY KEY,
  school_year INTEGER NOT NULL,
  grade INTEGER NOT NULL CHECK (grade IN (1, 2)),
  name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  archived_at TEXT
);

CREATE TABLE students (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  number INTEGER NOT NULL,
  name TEXT NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  UNIQUE (class_id, number)
);

CREATE TABLE observations (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  observed_at TEXT NOT NULL,
  topic_book_id TEXT,
  note TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE INDEX idx_observations_class_time ON observations(class_id, observed_at);

CREATE TABLE observation_students (
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  PRIMARY KEY (observation_id, student_id)
);
CREATE INDEX idx_obs_students_student ON observation_students(student_id);

CREATE TABLE observation_standards (
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  standard_code TEXT NOT NULL,
  PRIMARY KEY (observation_id, standard_code)
);

CREATE TABLE observation_tags (
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  tag TEXT NOT NULL,
  PRIMARY KEY (observation_id, tag)
);

CREATE TABLE attachments (
  id TEXT PRIMARY KEY,
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('photo', 'audio')),
  rel_path TEXT NOT NULL,
  mime TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  transcript TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#];

pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

pub fn apply(conn: &Connection) -> DbResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).db()?;
    if current > latest_version() {
        return Err("이 데이터 파일은 더 새로운 앱 버전에서 만들어졌습니다. 앱을 업데이트해 주세요".to_string());
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction().db()?;
        tx.execute_batch(sql).db()?;
        // PRAGMA는 바인딩을 받지 않으므로 숫자를 직접 넣는다.
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1)).db()?;
        tx.commit().db()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_and_sets_user_version() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, latest_version());
        assert_eq!(v, 1);
        for table in ["classes", "students", "observations", "observation_students", "observation_standards", "observation_tags", "attachments", "settings"] {
            let n: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "{table}");
        }
    }

    #[test]
    fn is_idempotent_and_rejects_newer() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();
        apply(&conn).unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        assert!(apply(&conn).is_err());
    }
}
