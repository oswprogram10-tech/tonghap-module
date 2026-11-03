//! 관찰 기록: 작성·수정·삭제(휴지통)·목록.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::attachments::{self, Attachment};
use super::{lock, new_id, now_iso, DbExt, DbResult, MediaDir};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationInput {
    pub class_id: String,
    pub observed_at: String,
    pub topic_book_id: Option<String>,
    #[serde(default)]
    pub note: String,
    pub student_ids: Vec<String>,
    #[serde(default)]
    pub standard_codes: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 연결할 단원 차시(선택). 같은 학급 단원의 차시여야 한다.
    #[serde(default)]
    pub unit_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ObservationStudent {
    pub id: String,
    pub number: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub id: String,
    pub class_id: String,
    pub observed_at: String,
    pub topic_book_id: Option<String>,
    pub note: String,
    pub student_ids: Vec<String>,
    pub standard_codes: Vec<String>,
    pub tags: Vec<String>,
    pub unit_session_id: Option<String>,
    pub unit_session_title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub students: Vec<ObservationStudent>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationQuery {
    pub class_id: String,
    pub student_id: Option<String>,
    pub topic_book_id: Option<String>,
    pub standard_code: Option<String>,
    /// 삭제된 기록도 함께 돌려준다.
    #[serde(default)]
    pub include_deleted: bool,
    /// 휴지통 보기: 삭제된 기록만 돌려준다.
    #[serde(default)]
    pub deleted_only: bool,
    pub limit: Option<i64>,
    /// 이 `observedAt`보다 이전 기록만(페이지 넘김).
    pub before: Option<String>,
}

/// 주제책 id 형식 검사. 빈 값은 `None`. 단원 빌더와 공유한다.
pub fn check_topic_book_id(topic: Option<&str>) -> DbResult<Option<String>> {
    match topic.map(str::trim) {
        None | Some("") => Ok(None),
        Some(t) => {
            let re = Regex::new(r"^g[1-6]s[12]-[a-z0-9-]+$").expect("정규식");
            if !re.is_match(t) {
                return Err(format!("주제책 id 형식이 올바르지 않습니다: {t}"));
            }
            Ok(Some(t.to_string()))
        }
    }
}

/// 성취기준 코드 형식 검사(앞뒤 공백 제거한 코드를 돌려준다). 단원 빌더와 공유한다.
pub fn check_standard_code(code: &str) -> DbResult<String> {
    let code = code.trim();
    let re = Regex::new(r"^\[2.{1,2}\d{2}-\d{2}\]$").expect("정규식");
    if !re.is_match(code) {
        return Err(format!("성취기준 코드 형식이 올바르지 않습니다: {code}"));
    }
    Ok(code.to_string())
}

const MAX_TAG_CHARS: usize = 20;
const MAX_NOTE_CHARS: usize = 10_000;

fn validate(conn: &Connection, input: &ObservationInput) -> DbResult<ObservationInput> {
    let class_exists: i64 = conn.query_row("SELECT count(*) FROM classes WHERE id = ?1", [&input.class_id], |r| r.get(0)).db()?;
    if class_exists == 0 {
        return Err("학급을 찾을 수 없습니다".to_string());
    }
    chrono::DateTime::parse_from_rfc3339(&input.observed_at).map_err(|_| "관찰 시각 형식이 올바르지 않습니다".to_string())?;
    if input.note.chars().count() > MAX_NOTE_CHARS {
        return Err("메모가 너무 깁니다".to_string());
    }
    let topic = check_topic_book_id(input.topic_book_id.as_deref())?;
    let unit_session_id = match input.unit_session_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(id) => {
            let ok: i64 = conn
                .query_row(
                    "SELECT count(*) FROM unit_sessions s JOIN units u ON u.id = s.unit_id WHERE s.id = ?1 AND u.class_id = ?2",
                    params![id, input.class_id],
                    |r| r.get(0),
                )
                .db()?;
            if ok == 0 {
                return Err("선택한 차시가 이 학급의 단원에 없습니다".to_string());
            }
            Some(id.to_string())
        }
    };

    let mut student_ids: Vec<String> = Vec::new();
    for id in &input.student_ids {
        if !student_ids.contains(id) {
            student_ids.push(id.clone());
        }
    }
    if student_ids.is_empty() {
        return Err("학생을 한 명 이상 선택해 주세요".to_string());
    }
    for id in &student_ids {
        let ok: i64 = conn
            .query_row("SELECT count(*) FROM students WHERE id = ?1 AND class_id = ?2", params![id, input.class_id], |r| r.get(0))
            .db()?;
        if ok == 0 {
            return Err("선택한 학생이 이 학급에 없습니다".to_string());
        }
    }

    let mut standard_codes: Vec<String> = Vec::new();
    for code in &input.standard_codes {
        let code = check_standard_code(code)?;
        if !standard_codes.contains(&code) {
            standard_codes.push(code);
        }
    }

    let mut tags: Vec<String> = Vec::new();
    for tag in &input.tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(format!("태그가 너무 깁니다: {tag}"));
        }
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.to_string());
        }
    }

    Ok(ObservationInput {
        class_id: input.class_id.clone(),
        observed_at: input.observed_at.clone(),
        topic_book_id: topic,
        note: input.note.clone(),
        student_ids,
        standard_codes,
        tags,
        unit_session_id,
    })
}

fn replace_children(conn: &Connection, id: &str, input: &ObservationInput) -> DbResult<()> {
    for table in ["observation_students", "observation_standards", "observation_tags"] {
        conn.execute(&format!("DELETE FROM {table} WHERE observation_id = ?1"), [id]).db()?;
    }
    for s in &input.student_ids {
        conn.execute("INSERT INTO observation_students (observation_id, student_id) VALUES (?1, ?2)", params![id, s]).db()?;
    }
    for c in &input.standard_codes {
        conn.execute("INSERT INTO observation_standards (observation_id, standard_code) VALUES (?1, ?2)", params![id, c]).db()?;
    }
    for t in &input.tags {
        conn.execute("INSERT INTO observation_tags (observation_id, tag) VALUES (?1, ?2)", params![id, t]).db()?;
    }
    Ok(())
}

pub fn get(conn: &Connection, media: &Path, id: &str) -> DbResult<Observation> {
    let mut o = conn
        .query_row(
            "SELECT o.id, o.class_id, o.observed_at, o.topic_book_id, o.note, o.created_at, o.updated_at, o.deleted_at,
                    o.unit_session_id, us.title
             FROM observations o LEFT JOIN unit_sessions us ON us.id = o.unit_session_id WHERE o.id = ?1",
            [id],
            |r| {
                Ok(Observation {
                    id: r.get(0)?,
                    class_id: r.get(1)?,
                    observed_at: r.get(2)?,
                    topic_book_id: r.get(3)?,
                    note: r.get(4)?,
                    student_ids: vec![],
                    standard_codes: vec![],
                    tags: vec![],
                    unit_session_id: r.get(8)?,
                    unit_session_title: r.get(9)?,
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                    deleted_at: r.get(7)?,
                    students: vec![],
                    attachments: vec![],
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "관찰 기록을 찾을 수 없습니다".to_string(),
            e => format!("데이터베이스 오류: {e}"),
        })?;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.number, s.name FROM observation_students os
             JOIN students s ON s.id = os.student_id WHERE os.observation_id = ?1 ORDER BY s.number",
        )
        .db()?;
    o.students = stmt
        .query_map([id], |r| Ok(ObservationStudent { id: r.get(0)?, number: r.get(1)?, name: r.get(2)? }))
        .db()?
        .collect::<Result<_, _>>()
        .db()?;
    o.student_ids = o.students.iter().map(|s| s.id.clone()).collect();
    let strings = |sql: &str| -> DbResult<Vec<String>> {
        let mut stmt = conn.prepare(sql).db()?;
        let rows = stmt.query_map([id], |r| r.get(0)).db()?;
        rows.collect::<Result<_, _>>().db()
    };
    o.standard_codes = strings("SELECT standard_code FROM observation_standards WHERE observation_id = ?1 ORDER BY standard_code")?;
    o.tags = strings("SELECT tag FROM observation_tags WHERE observation_id = ?1 ORDER BY rowid")?;
    o.attachments = attachments::list_for(conn, media, id)?;
    Ok(o)
}

pub fn create(conn: &mut Connection, media: &Path, input: &ObservationInput) -> DbResult<Observation> {
    let input = validate(conn, input)?;
    let id = new_id();
    let now = now_iso();
    let tx = conn.transaction().db()?;
    tx.execute(
        "INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at, unit_session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
        params![id, input.class_id, input.observed_at, input.topic_book_id, input.note, now, input.unit_session_id],
    )
    .db()?;
    replace_children(&tx, &id, &input)?;
    tx.commit().db()?;
    get(conn, media, &id)
}

pub fn update(conn: &mut Connection, media: &Path, id: &str, input: &ObservationInput) -> DbResult<Observation> {
    let existing = get(conn, media, id)?;
    if existing.deleted_at.is_some() {
        return Err("삭제된 기록은 수정할 수 없습니다. 먼저 복원해 주세요".to_string());
    }
    if existing.class_id != input.class_id {
        return Err("기록의 학급은 바꿀 수 없습니다".to_string());
    }
    let input = validate(conn, input)?;
    let tx = conn.transaction().db()?;
    tx.execute(
        "UPDATE observations SET observed_at = ?2, topic_book_id = ?3, note = ?4, updated_at = ?5, unit_session_id = ?6 WHERE id = ?1",
        params![id, input.observed_at, input.topic_book_id, input.note, now_iso(), input.unit_session_id],
    )
    .db()?;
    replace_children(&tx, id, &input)?;
    tx.commit().db()?;
    get(conn, media, id)
}

fn set_deleted(conn: &Connection, id: &str, deleted: bool) -> DbResult<()> {
    let at = deleted.then(now_iso);
    let n = conn.execute("UPDATE observations SET deleted_at = ?2 WHERE id = ?1", params![id, at]).db()?;
    if n == 0 {
        return Err("관찰 기록을 찾을 수 없습니다".to_string());
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> DbResult<()> {
    set_deleted(conn, id, true)
}

pub fn restore(conn: &Connection, id: &str) -> DbResult<()> {
    set_deleted(conn, id, false)
}

/// 완전 삭제. 첨부 파일도 디스크에서 지운다.
pub fn purge(conn: &Connection, media: &Path, id: &str) -> DbResult<()> {
    let paths: Vec<String> = {
        let mut stmt = conn.prepare("SELECT rel_path FROM attachments WHERE observation_id = ?1").db()?;
        let rows = stmt.query_map([id], |r| r.get(0)).db()?;
        rows.collect::<Result<_, _>>().db()?
    };
    let n = conn.execute("DELETE FROM observations WHERE id = ?1", [id]).db()?;
    if n == 0 {
        return Err("관찰 기록을 찾을 수 없습니다".to_string());
    }
    for rel in paths {
        let _ = std::fs::remove_file(attachments::abs_path(media, &rel));
    }
    Ok(())
}

pub fn list(conn: &Connection, media: &Path, q: &ObservationQuery) -> DbResult<Vec<Observation>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let mut stmt = conn
        .prepare(
            "SELECT o.id FROM observations o
             WHERE o.class_id = ?1
               AND (?2 IS NULL OR EXISTS (SELECT 1 FROM observation_students os WHERE os.observation_id = o.id AND os.student_id = ?2))
               AND (?3 IS NULL OR o.topic_book_id = ?3)
               AND (?4 IS NULL OR EXISTS (SELECT 1 FROM observation_standards st WHERE st.observation_id = o.id AND st.standard_code = ?4))
               AND (CASE WHEN ?5 THEN o.deleted_at IS NOT NULL WHEN ?6 THEN 1 ELSE o.deleted_at IS NULL END)
               AND (?7 IS NULL OR o.observed_at < ?7)
             ORDER BY o.observed_at DESC, o.created_at DESC, o.id
             LIMIT ?8",
        )
        .db()?;
    let ids: Vec<String> = stmt
        .query_map(
            params![q.class_id, q.student_id, q.topic_book_id, q.standard_code, q.deleted_only, q.include_deleted, q.before, limit],
            |r| r.get(0),
        )
        .db()?
        .collect::<Result<_, _>>()
        .db()?;
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(id.clone())).map(|id| get(conn, media, &id)).collect()
}

#[tauri::command]
pub fn observation_create(db: State<Mutex<Connection>>, media: State<MediaDir>, input: ObservationInput) -> DbResult<Observation> {
    create(&mut *lock(&db)?, &media.0, &input)
}

#[tauri::command]
pub fn observation_update(db: State<Mutex<Connection>>, media: State<MediaDir>, id: String, input: ObservationInput) -> DbResult<Observation> {
    update(&mut *lock(&db)?, &media.0, &id, &input)
}

#[tauri::command]
pub fn observation_delete(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    delete(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn observation_restore(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    restore(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn observation_purge(db: State<Mutex<Connection>>, media: State<MediaDir>, id: String) -> DbResult<()> {
    purge(&*lock(&db)?, &media.0, &id)
}

#[tauri::command]
pub fn observation_list(db: State<Mutex<Connection>>, media: State<MediaDir>, query: ObservationQuery) -> DbResult<Vec<Observation>> {
    list(&*lock(&db)?, &media.0, &query)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::{classes, students, test_conn};

    pub fn media() -> std::path::PathBuf {
        std::env::temp_dir().join("tonghap-test-unused-media")
    }

    pub fn input(class: &str, students: &[&str]) -> ObservationInput {
        ObservationInput {
            class_id: class.to_string(),
            observed_at: "2027-03-05T10:20:00+09:00".to_string(),
            topic_book_id: Some("g1s1-school".to_string()),
            note: "친구를 도와줌".to_string(),
            student_ids: students.iter().map(|s| s.to_string()).collect(),
            standard_codes: vec!["[2바01-01]".to_string()],
            tags: vec!["협력".to_string()],
            unit_session_id: None,
        }
    }

    /// 학급 1개, 학생 3명, 관찰 1개(학생 1·2). (conn, class_id, student_ids, observation_id)
    pub fn seed() -> (Connection, String, Vec<String>, String) {
        let mut conn = test_conn();
        let c = classes::create(&conn, 2027, 1, "1반").unwrap();
        students::import(&mut conn, &c.id, "1 가\n2 나\n3 다").unwrap();
        let ids: Vec<String> = students::list(&conn, &c.id, true).unwrap().into_iter().map(|s| s.id).collect();
        let o = create(&mut conn, &media(), &input(&c.id, &[&ids[0], &ids[1]])).unwrap();
        (conn, c.id, ids, o.id)
    }

    #[test]
    fn create_returns_full_observation() {
        let (conn, cid, ids, oid) = seed();
        let o = get(&conn, &media(), &oid).unwrap();
        assert_eq!(o.class_id, cid);
        assert_eq!(o.students.iter().map(|s| s.number).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(o.student_ids, ids[..2]);
        assert_eq!(o.standard_codes, ["[2바01-01]"]);
        assert_eq!(o.tags, ["협력"]);
        assert!(o.deleted_at.is_none());
    }

    #[test]
    fn update_replaces_children() {
        let (mut conn, cid, ids, oid) = seed();
        let mut inp = input(&cid, &[&ids[2]]);
        inp.standard_codes = vec!["[2슬02-04]".into(), "[2즐01-02]".into(), "[2슬02-04]".into()];
        inp.tags = vec!["탐구".into(), "놀이".into()];
        inp.topic_book_id = None;
        inp.note = "바뀜".into();
        let o = update(&mut conn, &media(), &oid, &inp).unwrap();
        assert_eq!(o.student_ids, [ids[2].clone()]);
        assert_eq!(o.standard_codes.len(), 2);
        assert_eq!(o.tags, ["탐구", "놀이"]);
        assert_eq!((o.topic_book_id, o.note.as_str()), (None, "바뀜"));
    }

    #[test]
    fn rejects_other_class_students_empty_students_and_bad_codes() {
        let (mut conn, cid, ids, _) = seed();
        let other = classes::create(&conn, 2027, 2, "2반").unwrap();
        students::import(&mut conn, &other.id, "1 외부").unwrap();
        let foreign = students::list(&conn, &other.id, true).unwrap().remove(0).id;
        assert!(create(&mut conn, &media(), &input(&cid, &[&ids[0], &foreign])).unwrap_err().contains("학급에 없습니다"));
        assert!(create(&mut conn, &media(), &input(&cid, &[])).unwrap_err().contains("한 명 이상"));
        for bad in ["2바01-01", "[3바01-01]", "[2바1-01]", "[2바01-1]", "[2바바바01-01]"] {
            let mut inp = input(&cid, &[&ids[0]]);
            inp.standard_codes = vec![bad.to_string()];
            assert!(create(&mut conn, &media(), &inp).is_err(), "{bad}");
        }
        let mut inp = input(&cid, &[&ids[0]]);
        inp.topic_book_id = Some("G1S1-School".into());
        assert!(create(&mut conn, &media(), &inp).is_err());
        inp.topic_book_id = Some("g7s1-x".into());
        assert!(create(&mut conn, &media(), &inp).is_err());
    }

    #[test]
    fn delete_restore_and_list_filtering() {
        let (conn, cid, ids, oid) = seed();
        let q = ObservationQuery { class_id: cid.clone(), ..Default::default() };
        assert_eq!(list(&conn, &media(), &q).unwrap().len(), 1);
        delete(&conn, &oid).unwrap();
        assert!(list(&conn, &media(), &q).unwrap().is_empty());
        let trash = ObservationQuery { class_id: cid.clone(), deleted_only: true, ..Default::default() };
        assert_eq!(list(&conn, &media(), &trash).unwrap().len(), 1);
        let all = ObservationQuery { class_id: cid.clone(), include_deleted: true, ..Default::default() };
        assert_eq!(list(&conn, &media(), &all).unwrap().len(), 1);
        restore(&conn, &oid).unwrap();
        assert_eq!(list(&conn, &media(), &q).unwrap().len(), 1);
        let by_student = ObservationQuery { class_id: cid.clone(), student_id: Some(ids[2].clone()), ..Default::default() };
        assert!(list(&conn, &media(), &by_student).unwrap().is_empty());
        let by_std = ObservationQuery { class_id: cid, standard_code: Some("[2바01-01]".into()), ..Default::default() };
        assert_eq!(list(&conn, &media(), &by_std).unwrap().len(), 1);
    }

    #[test]
    fn list_paginates_with_before_and_limit() {
        let (mut conn, cid, ids, _) = seed();
        for day in 6..=9 {
            let mut inp = input(&cid, &[&ids[0]]);
            inp.observed_at = format!("2027-03-0{day}T10:00:00+09:00");
            create(&mut conn, &media(), &inp).unwrap();
        }
        let q = ObservationQuery { class_id: cid.clone(), limit: Some(2), ..Default::default() };
        let p1 = list(&conn, &media(), &q).unwrap();
        assert_eq!(p1.len(), 2);
        assert!(p1[0].observed_at > p1[1].observed_at);
        let q2 = ObservationQuery { class_id: cid, limit: Some(10), before: Some(p1[1].observed_at.clone()), ..Default::default() };
        assert_eq!(list(&conn, &media(), &q2).unwrap().len(), 3);
    }

    #[test]
    fn purge_removes_attachment_rows_and_files() {
        let (conn, _cid, _ids, oid) = seed();
        let media = std::env::temp_dir().join(format!("tonghap-test-purge-{}", new_id()));
        let a = attachments::add_bytes(&conn, &media, &oid, "audio", "audio/webm", b"zzz").unwrap();
        assert!(Path::new(&a.abs_path).exists());
        purge(&conn, &media, &oid).unwrap();
        assert!(!Path::new(&a.abs_path).exists());
        let n: i64 = conn.query_row("SELECT count(*) FROM attachments", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        let n: i64 = conn.query_row("SELECT count(*) FROM observation_students", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert!(purge(&conn, &media, &oid).is_err());
    }
}
