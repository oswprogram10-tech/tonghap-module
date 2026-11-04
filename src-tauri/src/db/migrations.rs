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
    // 3: 칠판 모드(사용자 놀이, 놀이 기록, 습관 신호등)
    r#"
CREATE TABLE custom_plays (
  id TEXT PRIMARY KEY,
  data TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE play_log (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  play_id TEXT NOT NULL,
  played_at TEXT NOT NULL,
  minutes INTEGER
);
CREATE INDEX idx_play_log_class ON play_log(class_id, played_at);

CREATE TABLE habits (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  icon TEXT NOT NULL DEFAULT '',
  starts_on TEXT,
  ends_on TEXT,
  active INTEGER NOT NULL DEFAULT 1,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_habits_class ON habits(class_id);

CREATE TABLE habit_checks (
  habit_id TEXT NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  checked_on TEXT NOT NULL,
  done INTEGER NOT NULL CHECK (done IN (0, 1)),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (habit_id, student_id, checked_on)
);
CREATE INDEX idx_habit_checks_day ON habit_checks(habit_id, checked_on);
"#,
    // 4: 음성 메모 자동 자막 상태
    r#"
ALTER TABLE attachments ADD COLUMN transcript_status TEXT NOT NULL DEFAULT 'none'
  CHECK (transcript_status IN ('none', 'queued', 'running', 'done', 'failed'));
ALTER TABLE attachments ADD COLUMN transcript_error TEXT;
ALTER TABLE attachments ADD COLUMN transcript_model TEXT;
ALTER TABLE attachments ADD COLUMN transcript_edited INTEGER NOT NULL DEFAULT 0;
ALTER TABLE attachments ADD COLUMN transcribed_at TEXT;
"#,
    // 5: 학부모 리포트
    r#"
CREATE TABLE parent_reports (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  topic_book_id TEXT,
  period_from TEXT NOT NULL,
  period_to TEXT NOT NULL,
  message TEXT NOT NULL DEFAULT '',
  observation_ids TEXT NOT NULL DEFAULT '[]',
  attachment_ids TEXT NOT NULL DEFAULT '[]',
  quote_attachment_ids TEXT NOT NULL DEFAULT '[]',
  exported_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (student_id, topic_book_id, period_from, period_to)
);
CREATE INDEX idx_parent_reports_class ON parent_reports(class_id);
"#,
    // 6: 생기부 평어 초안
    r#"
CREATE TABLE evaluation_drafts (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  subject TEXT NOT NULL CHECK (subject IN ('바', '슬', '즐')),
  period_from TEXT NOT NULL,
  period_to TEXT NOT NULL,
  sentences TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'confirmed')),
  generator TEXT NOT NULL DEFAULT 'template',
  confirmed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (student_id, subject, period_from, period_to)
);
CREATE INDEX idx_evaluation_drafts_class ON evaluation_drafts(class_id, subject);
"#,
    // 7: 활동 카드(교사 카드, 차시가 온 카드)
    r#"
CREATE TABLE custom_activities (
  id TEXT PRIMARY KEY,
  data TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

ALTER TABLE unit_sessions ADD COLUMN activity_id TEXT;
"#,
    // 8: 1년치 성능. attachments에는 observation_id 인덱스가 없어, 관찰마다 첨부를 찾는 쿼리가
    // (기록 찾기의 자막 검색, `observations::get`의 첨부 목록 -> 기록 목록·리포트·평어 근거 묶음) 첨부 전체를 훑었다.
    // 첨부 1,500개·관찰 3,000건에서 검색어 + 기간 개수가 150ms를 넘었다(EXPLAIN QUERY PLAN의 `SCAN a`).
    r#"
CREATE INDEX idx_attachments_observation ON attachments(observation_id);
"#,
    // 9: 세 교과(바·슬·즐)를 합친 통합 평어. 1~2학년은 NEIS에 세 교과를 통합해 입력한다.
    r#"
CREATE TABLE evaluation_combined (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  period_from TEXT NOT NULL,
  period_to TEXT NOT NULL,
  text TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'confirmed')),
  confirmed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (student_id, period_from, period_to)
);
CREATE INDEX idx_evaluation_combined_class ON evaluation_combined(class_id);
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
        assert_eq!(v, 9);
        for table in [
            "classes", "students", "observations", "observation_students", "observation_standards", "observation_tags", "attachments", "settings",
            "units", "unit_stages", "unit_sessions", "unit_session_standards", "custom_plays", "play_log", "habits", "habit_checks", "parent_reports",
            "evaluation_drafts", "custom_activities", "evaluation_combined",
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
        apply_up_to(&conn, 2).unwrap();
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

    #[test]
    fn migration_3_keeps_version_2_data_and_adds_board_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 2).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO units (id, class_id, tool_id, title, created_at, updated_at) VALUES ('u', 'c', 'free', '단원', 't', 't');",
        )
        .unwrap();
        apply_up_to(&conn, 3).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 3);
        let title: String = conn.query_row("SELECT title FROM units WHERE id = 'u'", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "단원");
        for table in ["custom_plays", "play_log", "habits", "habit_checks"] {
            let n: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0)).unwrap();
            assert_eq!(n, 1, "{table}");
        }
    }

    #[test]
    fn migration_4_keeps_version_3_attachments_and_adds_transcript_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 3).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at)
               VALUES ('o', 'c', '2027-03-05T10:00:00+09:00', 'g1s1-school', '메모', 't', 't');
             INSERT INTO attachments (id, observation_id, kind, rel_path, mime, bytes, transcript, created_at)
               VALUES ('a', 'o', 'audio', '2027/03/a.webm', 'audio/webm', 10, '옛 자막', 't');",
        )
        .unwrap();
        apply_up_to(&conn, 4).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 4);
        let row: (String, String, Option<String>, Option<String>, i64, Option<String>) = conn
            .query_row(
                "SELECT transcript, transcript_status, transcript_error, transcript_model, transcript_edited, transcribed_at FROM attachments WHERE id = 'a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .unwrap();
        assert_eq!(row, ("옛 자막".to_string(), "none".to_string(), None, None, 0, None));
        assert!(conn.execute("UPDATE attachments SET transcript_status = 'bogus' WHERE id = 'a'", []).is_err());
    }

    #[test]
    fn migration_5_keeps_version_4_data_and_adds_parent_reports() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 4).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s', 'c', 1, '김하늘', 't');",
        )
        .unwrap();
        apply_up_to(&conn, 5).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 5);
        let name: String = conn.query_row("SELECT name FROM students WHERE id = 's'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "김하늘");
        conn.execute(
            "INSERT INTO parent_reports (id, class_id, student_id, period_from, period_to, created_at, updated_at)
             VALUES ('r', 'c', 's', '2027-03-01', '2027-04-01', 't', 't')",
            [],
        )
        .unwrap();
        // 학생을 지우면 리포트도 함께 지워진다.
        conn.execute("DELETE FROM students WHERE id = 's'", []).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM parent_reports", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn migration_6_keeps_version_5_data_and_adds_evaluation_drafts() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 5).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s', 'c', 1, '김하늘', 't');",
        )
        .unwrap();
        apply_up_to(&conn, 6).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 6);
        let name: String = conn.query_row("SELECT name FROM students WHERE id = 's'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "김하늘");
        let insert = "INSERT INTO evaluation_drafts (id, class_id, student_id, subject, period_from, period_to, created_at, updated_at)
                      VALUES (?1, 'c', 's', ?2, '2027-03-02', '2027-07-18', 't', 't')";
        conn.execute(insert, ["d1", "슬"]).unwrap();
        // 같은 학생·교과·기간은 하나만, 알 수 없는 교과·상태는 거부.
        assert!(conn.execute(insert, ["d2", "슬"]).is_err());
        assert!(conn.execute(insert, ["d3", "수"]).is_err());
        assert!(conn.execute("UPDATE evaluation_drafts SET status = 'bogus' WHERE id = 'd1'", []).is_err());
        // 학생을 지우면 초안도 함께 지워진다.
        conn.execute("DELETE FROM students WHERE id = 's'", []).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn migration_7_keeps_version_6_sessions_and_adds_activity_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 6).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO units (id, class_id, tool_id, title, created_at, updated_at) VALUES ('u', 'c', 'free', '단원', 't', 't');
             INSERT INTO unit_stages (id, unit_id, position, title) VALUES ('g', 'u', 0, '수업');
             INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, created_at, updated_at) VALUES ('s', 'u', 'g', 0, '옛 차시', 't', 't');",
        )
        .unwrap();
        apply_up_to(&conn, 7).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 7);
        let (title, activity): (String, Option<String>) =
            conn.query_row("SELECT title, activity_id FROM unit_sessions WHERE id = 's'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((title.as_str(), activity), ("옛 차시", None));
        conn.execute("INSERT INTO custom_activities (id, data, created_at, updated_at) VALUES ('custom-1', '{}', 't', 't')", []).unwrap();
        assert!(conn.execute("INSERT INTO custom_activities (id, data, created_at, updated_at) VALUES ('custom-1', '{}', 't', 't')", []).is_err());
    }

    #[test]
    fn migration_8_keeps_version_7_data_and_indexes_attachments_by_observation() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 7).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO observations (id, class_id, observed_at, note, created_at, updated_at) VALUES ('o', 'c', '2027-03-04T10:00:00+09:00', '옛 기록', 't', 't');
             INSERT INTO attachments (id, observation_id, kind, rel_path, mime, bytes, created_at) VALUES ('a', 'o', 'photo', '2027/03/a.png', 'image/png', 3, 't');",
        )
        .unwrap();
        apply_up_to(&conn, 8).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, 8);
        let n: i64 = conn.query_row("SELECT count(*) FROM attachments WHERE observation_id = 'o'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // 인덱스를 실제로 쓴다(전체 훑기 `SCAN a`가 아니다).
        let plan: String = conn
            .query_row("EXPLAIN QUERY PLAN SELECT id FROM attachments WHERE observation_id = 'o'", [], |r| r.get(3))
            .unwrap();
        assert!(plan.contains("idx_attachments_observation"), "{plan}");
    }

    #[test]
    fn migration_9_keeps_version_8_data_and_adds_combined_evaluations() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_up_to(&conn, 8).unwrap();
        conn.execute_batch(
            "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't');
             INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s', 'c', 1, '김하늘', 't');
             INSERT INTO evaluation_drafts (id, class_id, student_id, subject, period_from, period_to, sentences, created_at, updated_at)
               VALUES ('d', 'c', 's', '슬', '2027-03-02', '2027-07-18', '[]', 't', 't');",
        )
        .unwrap();
        apply(&conn).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, latest_version());
        assert_eq!(latest_version(), 9);
        let n: i64 = conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let insert = "INSERT INTO evaluation_combined (id, class_id, student_id, period_from, period_to, text, source_hash, created_at, updated_at)
                      VALUES (?1, 'c', 's', '2027-03-02', '2027-07-18', '글', 'h', 't', 't')";
        conn.execute(insert, ["a"]).unwrap();
        assert!(conn.execute(insert, ["b"]).is_err(), "같은 학생·기간은 하나만");
        assert!(conn.execute("UPDATE evaluation_combined SET status = 'bogus' WHERE id = 'a'", []).is_err());
        // 학생을 지우면 통합본도 함께 지워진다.
        conn.execute("DELETE FROM students WHERE id = 's'", []).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM evaluation_combined", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
