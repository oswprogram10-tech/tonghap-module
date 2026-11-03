//! `PRAGMA user_version`으로 버전을 관리하는 순차 마이그레이션.

use rusqlite::Connection;

use super::{DbExt, DbResult};

/// 인덱스 0이 버전 1로 가는 SQL. 이미 배포된 항목은 고치지 말고 뒤에 추가한다.
const MIGRATIONS: &[&str] = &[
    r#"
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
"#,
    // 2: 단원 빌더
    r#"
CREATE TABLE units (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  topic_book_id TEXT,
  tool_id TEXT NOT NULL,
  title TEXT NOT NULL,
  memo TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE INDEX idx_units_class ON units(class_id);

CREATE TABLE unit_stages (
  id TEXT PRIMARY KEY,
  unit_id TEXT NOT NULL REFERENCES units(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  title TEXT NOT NULL
);

CREATE TABLE unit_sessions (
  id TEXT PRIMARY KEY,
  unit_id TEXT NOT NULL REFERENCES units(id) ON DELETE CASCADE,
  stage_id TEXT NOT NULL REFERENCES unit_stages(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  title TEXT NOT NULL,
  plan TEXT NOT NULL DEFAULT '',
  minutes INTEGER NOT NULL DEFAULT 40 CHECK (minutes > 0),
  kind TEXT NOT NULL DEFAULT 'general' CHECK (kind IN ('general', 'safety', 'play', 'makingbook')),
  scheduled_on TEXT,
  status TEXT NOT NULL DEFAULT 'planned' CHECK (status IN ('planned', 'done', 'skipped')),
  student_questions TEXT NOT NULL DEFAULT '',
  reflection TEXT NOT NULL DEFAULT '',
  follows_session_id TEXT REFERENCES unit_sessions(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_unit ON unit_sessions(unit_id);
CREATE INDEX idx_sessions_date ON unit_sessions(scheduled_on);

CREATE TABLE unit_session_standards (
  session_id TEXT NOT NULL REFERENCES unit_sessions(id) ON DELETE CASCADE,
  standard_code TEXT NOT NULL,
  mode TEXT NOT NULL CHECK (mode IN ('repeat', 'focus', 'emergent')),
  PRIMARY KEY (session_id, standard_code)
);

ALTER TABLE observations ADD COLUMN unit_session_id TEXT REFERENCES unit_sessions(id) ON DELETE SET NULL;
CREATE INDEX idx_observations_session ON observations(unit_session_id);
"#,
];

pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

pub fn apply(conn: &Connection) -> DbResult<()> {
    apply_up_to(conn, latest_version())
}

/// `version`까지만 올린다. 이전 버전 DB를 만드는 테스트에서도 쓴다.
pub fn apply_up_to(conn: &Connection, version: i64) -> DbResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).db()?;
    if current > latest_version() {
        return Err("이 데이터 파일은 더 새로운 앱 버전에서 만들어졌습니다. 앱을 업데이트해 주세요".to_string());
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().take(version as usize).skip(current as usize) {
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
        assert_eq!(v, 2);
        for table in [
            "classes", "students", "observations", "observation_students", "observation_standards", "observation_tags", "attachments", "settings",
            "units", "unit_stages", "unit_sessions", "unit_session_standards",
        ] {
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

    #[test]
    fn migration_2_keeps_version_1_data_and_adds_session_column() {
        let conn = Connection::open_in_memory().unwrap();
        apply_up_to(&conn, 1).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at)
               VALUES ('o', 'c', '2027-03-05T10:00:00+09:00', 'g1s1-school', '메모', 't', 't');",
        )
        .unwrap();
        apply(&conn).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 2);
        let (note, session): (String, Option<String>) =
            conn.query_row("SELECT note, unit_session_id FROM observations WHERE id = 'o'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((note.as_str(), session), ("메모", None));
        let has_col: i64 = conn
            .query_row("SELECT count(*) FROM pragma_table_info('observations') WHERE name = 'unit_session_id'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(has_col, 1);
    }
}
