//! 관찰 기록: 작성·수정·삭제(휴지통)·목록.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use regex::Regex;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
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
    /// 검색어가 있을 때만: 일치한 곳.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches: Option<Vec<SearchMatch>>,
}

/// 검색 일치 위치. `field`는 note | transcript | tag | session | student.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchMatch {
    pub field: String,
    pub snippet: String,
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
    /// 검색어(공백으로 나눈 낱말이 모두 들어 있어야 한다).
    pub text: Option<String>,
    /// 관찰 날짜 범위(`YYYY-MM-DD`, 양 끝 포함).
    pub from: Option<String>,
    pub to: Option<String>,
    pub has_photo: Option<bool>,
    pub has_audio: Option<bool>,
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
                    matches: None,
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

const MAX_TERMS: usize = 5;
const MAX_TERM_CHARS: usize = 30;
const SNIPPET_CONTEXT: usize = 20;

/// 검색어를 낱말로 나눈다(최대 5개, 낱말당 30자).
fn search_terms(text: Option<&str>) -> Vec<String> {
    text.map(|t| t.split_whitespace().take(MAX_TERMS).map(|w| w.chars().take(MAX_TERM_CHARS).collect()).collect()).unwrap_or_default()
}

fn like_pattern(term: &str) -> String {
    let mut out = String::from("%");
    for c in term.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

fn check_date(label: &str, d: &Option<String>) -> DbResult<Option<String>> {
    match d.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => {
            chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| format!("{label} 날짜 형식이 올바르지 않습니다"))?;
            Ok(Some(v.to_string()))
        }
    }
}

/// 인자를 추가하고 그 자리 번호(`?N`)를 돌려준다.
fn push_arg(args: &mut Vec<Value>, v: Value) -> usize {
    args.push(v);
    args.len()
}

/// 목록·개수가 함께 쓰는 WHERE 절과 인자. `paged`가 거짓이면 `before`를 무시한다.
fn build_where(q: &ObservationQuery, paged: bool) -> DbResult<(String, Vec<Value>)> {
    let mut args: Vec<Value> = Vec::new();
    let mut sql = String::new();
    let n = push_arg(&mut args, Value::Text(q.class_id.clone()));
    sql += &format!("o.class_id = ?{n}");
    if let Some(sid) = &q.student_id {
        let n = push_arg(&mut args, Value::Text(sid.clone()));
        sql += &format!(" AND EXISTS (SELECT 1 FROM observation_students os WHERE os.observation_id = o.id AND os.student_id = ?{n})");
    }
    if let Some(t) = &q.topic_book_id {
        let n = push_arg(&mut args, Value::Text(t.clone()));
        sql += &format!(" AND o.topic_book_id = ?{n}");
    }
    if let Some(c) = &q.standard_code {
        let n = push_arg(&mut args, Value::Text(c.clone()));
        sql += &format!(" AND EXISTS (SELECT 1 FROM observation_standards st WHERE st.observation_id = o.id AND st.standard_code = ?{n})");
    }
    sql += if q.deleted_only {
        " AND o.deleted_at IS NOT NULL"
    } else if q.include_deleted {
        ""
    } else {
        " AND o.deleted_at IS NULL"
    };
    if let (true, Some(b)) = (paged, &q.before) {
        let n = push_arg(&mut args, Value::Text(b.clone()));
        sql += &format!(" AND o.observed_at < ?{n}");
    }
    if let Some(from) = check_date("시작", &q.from)? {
        let n = push_arg(&mut args, Value::Text(from));
        sql += &format!(" AND substr(o.observed_at, 1, 10) >= ?{n}");
    }
    if let Some(to) = check_date("끝", &q.to)? {
        let n = push_arg(&mut args, Value::Text(to));
        sql += &format!(" AND substr(o.observed_at, 1, 10) <= ?{n}");
    }
    for (flag, kind) in [(q.has_photo, "photo"), (q.has_audio, "audio")] {
        if flag == Some(true) {
            sql += &format!(" AND EXISTS (SELECT 1 FROM attachments a WHERE a.observation_id = o.id AND a.kind = '{kind}')");
        }
    }
    for term in search_terms(q.text.as_deref()) {
        let n = push_arg(&mut args, Value::Text(like_pattern(&term)));
        sql += &format!(
            " AND (o.note LIKE ?{n} ESCAPE '\\'
               OR EXISTS (SELECT 1 FROM attachments a WHERE a.observation_id = o.id AND a.transcript LIKE ?{n} ESCAPE '\\')
               OR EXISTS (SELECT 1 FROM observation_tags t WHERE t.observation_id = o.id AND t.tag LIKE ?{n} ESCAPE '\\')
               OR EXISTS (SELECT 1 FROM unit_sessions us WHERE us.id = o.unit_session_id AND us.title LIKE ?{n} ESCAPE '\\')
               OR EXISTS (SELECT 1 FROM observation_students os JOIN students s ON s.id = os.student_id
                          WHERE os.observation_id = o.id AND s.name LIKE ?{n} ESCAPE '\\'))"
        );
    }
    Ok((sql, args))
}

fn fold(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// `text`에서 낱말이 처음 나오는 곳 앞뒤 20자를 잘라 돌려준다(문자 기준, 잘린 쪽에 말줄임표).
fn snippet_of(text: &str, terms: &[Vec<char>]) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let folded: Vec<char> = chars.iter().map(|&c| fold(c)).collect();
    let (pos, len) = terms
        .iter()
        .filter(|t| !t.is_empty() && t.len() <= folded.len())
        .filter_map(|t| folded.windows(t.len()).position(|w| w == t.as_slice()).map(|p| (p, t.len())))
        .min_by_key(|&(p, _)| p)?;
    let start = pos.saturating_sub(SNIPPET_CONTEXT);
    let end = (pos + len + SNIPPET_CONTEXT).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    Some(out)
}

fn fill_matches(o: &mut Observation, words: &[String]) {
    let terms: Vec<Vec<char>> = words.iter().map(|w| w.chars().map(fold).collect()).collect();
    let mut found: Vec<SearchMatch> = Vec::new();
    let mut add = |field: &str, text: &str| {
        if let Some(snippet) = snippet_of(text, &terms) {
            found.push(SearchMatch { field: field.to_string(), snippet });
        }
    };
    add("note", &o.note);
    for a in &o.attachments {
        if let Some(t) = &a.transcript {
            add("transcript", t);
        }
    }
    for t in &o.tags {
        add("tag", t);
    }
    if let Some(t) = &o.unit_session_title {
        add("session", t);
    }
    for s in &o.students {
        add("student", &s.name);
    }
    o.matches = Some(found);
}

pub fn list(conn: &Connection, media: &Path, q: &ObservationQuery) -> DbResult<Vec<Observation>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let (wh, mut args) = build_where(q, true)?;
    let n = push_arg(&mut args, Value::Integer(limit));
    let sql = format!(
        "SELECT o.id FROM observations o WHERE {wh}
         ORDER BY o.observed_at DESC, o.created_at DESC, o.id LIMIT ?{n}"
    );
    let mut stmt = conn.prepare(&sql).db()?;
    let ids: Vec<String> = stmt.query_map(params_from_iter(args), |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    let words = search_terms(q.text.as_deref());
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(id.clone()))
        .map(|id| {
            let mut o = get(conn, media, &id)?;
            if !words.is_empty() {
                fill_matches(&mut o, &words);
            }
            Ok(o)
        })
        .collect()
}

/// 같은 조건에 맞는 전체 개수(`before`·`limit` 무시).
pub fn count(conn: &Connection, q: &ObservationQuery) -> DbResult<i64> {
    let (wh, args) = build_where(q, false)?;
    conn.query_row(&format!("SELECT count(*) FROM observations o WHERE {wh}"), params_from_iter(args), |r| r.get(0)).db()
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

#[tauri::command]
pub fn observation_count(db: State<Mutex<Connection>>, query: ObservationQuery) -> DbResult<i64> {
    count(&*lock(&db)?, &query)
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

    // ---- 검색 ----

    fn q(cid: &str, text: &str) -> ObservationQuery {
        ObservationQuery { class_id: cid.to_string(), text: Some(text.to_string()), ..Default::default() }
    }

    fn found(conn: &Connection, query: &ObservationQuery) -> Vec<String> {
        list(conn, &media(), query).unwrap().into_iter().map(|o| o.id).collect()
    }

    fn add_obs(conn: &mut Connection, cid: &str, sid: &str, day: u32, note: &str, tags: &[&str]) -> Observation {
        let mut inp = input(cid, &[sid]);
        inp.observed_at = format!("2027-03-{day:02}T10:00:00+09:00");
        inp.note = note.to_string();
        inp.tags = tags.iter().map(|t| t.to_string()).collect();
        create(conn, &media(), &inp).unwrap()
    }

    #[test]
    fn search_by_note_transcript_tag_and_student_name() {
        let (mut conn, cid, ids, seed_id) = seed();
        let o = add_obs(&mut conn, &cid, &ids[2], 6, "블록을 쌓음", &["탐구"]);
        let media = std::env::temp_dir().join(format!("tonghap-test-search-{}", new_id()));
        let a = attachments::add_bytes(&conn, &media, &o.id, "audio", "audio/webm", b"zzz").unwrap();
        attachments::set_transcript(&conn, &media, &a.id, "소방차가 빨리 달려요").unwrap();
        // 메모(두 글자 낱말도 조사 앞에서 찾는다)
        assert_eq!(found(&conn, &q(&cid, "블록")), vec![o.id.clone()]);
        // 자막
        let r = list(&conn, &media, &q(&cid, "소방차")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].matches.as_ref().unwrap()[0].field, "transcript");
        // 태그
        assert_eq!(found(&conn, &q(&cid, "탐구")), vec![o.id.clone()]);
        // 학생 이름("다"는 학생 3)
        assert_eq!(found(&conn, &q(&cid, "다")), vec![o.id.clone()]);
        // 시드 기록은 메모 "친구를 도와줌"
        assert_eq!(found(&conn, &q(&cid, "도와")), vec![seed_id]);
        assert!(found(&conn, &q(&cid, "없는말")).is_empty());
        // 검색어 없으면 matches 없음
        let all = list(&conn, &media, &ObservationQuery { class_id: cid.clone(), text: Some("  ".into()), ..Default::default() }).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|o| o.matches.is_none()));
        let _ = std::fs::remove_dir_all(&media);
    }

    #[test]
    fn multiple_terms_are_anded_across_fields() {
        let (mut conn, cid, ids, seed_id) = seed();
        let o = add_obs(&mut conn, &cid, &ids[2], 6, "블록 놀이", &["협력"]);
        // 낱말마다 다른 곳(메모, 태그, 학생 이름)에서 일치
        assert_eq!(found(&conn, &q(&cid, "블록 협력 다")), vec![o.id.clone()]);
        // 하나라도 없으면 제외
        assert!(found(&conn, &q(&cid, "블록 없음")).is_empty());
        // "협력"은 두 기록 모두 태그에 있다
        assert_eq!(found(&conn, &q(&cid, "협력")).len(), 2);
        assert_eq!(found(&conn, &q(&cid, "협력 친구")), vec![seed_id]);
    }

    #[test]
    fn like_wildcards_are_literal() {
        let (mut conn, cid, ids, _) = seed();
        let a = add_obs(&mut conn, &cid, &ids[0], 6, "100% 완성", &[]);
        let b = add_obs(&mut conn, &cid, &ids[0], 7, "a_b 이름", &[]);
        let c = add_obs(&mut conn, &cid, &ids[0], 8, "경로 a\\b", &[]);
        assert_eq!(found(&conn, &q(&cid, "%")), vec![a.id.clone()]);
        assert_eq!(found(&conn, &q(&cid, "_")), vec![b.id.clone()]);
        assert_eq!(found(&conn, &q(&cid, "a_b")), vec![b.id.clone()]);
        assert_eq!(found(&conn, &q(&cid, "\\")), vec![c.id.clone()]);
        assert!(found(&conn, &q(&cid, "블%기")).is_empty());
    }

    #[test]
    fn term_limits_apply() {
        let (conn, cid, _, _) = seed();
        assert_eq!(search_terms(Some("a b c d e f g")).len(), 5);
        assert_eq!(search_terms(Some(&"가".repeat(40)))[0].chars().count(), 30);
        assert!(found(&conn, &q(&cid, "")).len() == 1);
    }

    #[test]
    fn date_range_is_inclusive_and_attachments_filter() {
        let (mut conn, cid, ids, _) = seed(); // 3/5
        let d6 = add_obs(&mut conn, &cid, &ids[0], 6, "육", &[]);
        let d7 = add_obs(&mut conn, &cid, &ids[0], 7, "칠", &[]);
        let d8 = add_obs(&mut conn, &cid, &ids[0], 8, "팔", &[]);
        let range = |from: &str, to: &str| ObservationQuery {
            class_id: cid.clone(),
            from: Some(from.into()),
            to: Some(to.into()),
            ..Default::default()
        };
        assert_eq!(found(&conn, &range("2027-03-06", "2027-03-08")), vec![d8.id.clone(), d7.id.clone(), d6.id.clone()]);
        assert_eq!(found(&conn, &range("2027-03-07", "2027-03-07")), vec![d7.id.clone()]);
        assert_eq!(found(&conn, &ObservationQuery { class_id: cid.clone(), from: Some("2027-03-08".into()), ..Default::default() }), vec![d8.id.clone()]);
        assert_eq!(found(&conn, &ObservationQuery { class_id: cid.clone(), to: Some("2027-03-05".into()), ..Default::default() }).len(), 1);
        assert!(list(&conn, &media(), &range("내일", "2027-03-08")).is_err());

        let media = std::env::temp_dir().join(format!("tonghap-test-search-att-{}", new_id()));
        attachments::add_bytes(&conn, &media, &d6.id, "audio", "audio/webm", b"a").unwrap();
        attachments::add_bytes(&conn, &media, &d7.id, "audio", "audio/webm", b"a").unwrap();
        // 사진은 파일 복사 경로가 필요하므로 행을 직접 넣는다.
        conn.execute(
            "INSERT INTO attachments (id, observation_id, kind, rel_path, mime, bytes, created_at) VALUES ('p1', ?1, 'photo', 'x.jpg', 'image/jpeg', 1, 'now')",
            [&d7.id],
        )
        .unwrap();
        let aud = ObservationQuery { class_id: cid.clone(), has_audio: Some(true), ..Default::default() };
        assert_eq!(found(&conn, &aud), vec![d7.id.clone(), d6.id.clone()]);
        let pho = ObservationQuery { class_id: cid.clone(), has_photo: Some(true), ..Default::default() };
        assert_eq!(found(&conn, &pho), vec![d7.id.clone()]);
        let both = ObservationQuery { class_id: cid.clone(), has_photo: Some(true), has_audio: Some(true), ..Default::default() };
        assert_eq!(found(&conn, &both), vec![d7.id.clone()]);
        let off = ObservationQuery { class_id: cid.clone(), has_photo: Some(false), ..Default::default() };
        assert_eq!(found(&conn, &off).len(), 4);
        let _ = std::fs::remove_dir_all(&media);
    }

    #[test]
    fn search_combines_with_existing_filters() {
        let (mut conn, cid, ids, _) = seed();
        let mut inp = input(&cid, &[&ids[2]]);
        inp.note = "공통 낱말".into();
        inp.topic_book_id = Some("g1s2-x".into());
        inp.standard_codes = vec!["[2슬02-04]".into()];
        let o = create(&mut conn, &media(), &inp).unwrap();
        let mut inp2 = input(&cid, &[&ids[0]]);
        inp2.note = "공통 낱말".into();
        create(&mut conn, &media(), &inp2).unwrap();
        let base = || q(&cid, "공통");
        assert_eq!(found(&conn, &base()).len(), 2);
        assert_eq!(found(&conn, &ObservationQuery { student_id: Some(ids[2].clone()), ..base() }), vec![o.id.clone()]);
        assert_eq!(found(&conn, &ObservationQuery { topic_book_id: Some("g1s2-x".into()), ..base() }), vec![o.id.clone()]);
        assert_eq!(found(&conn, &ObservationQuery { standard_code: Some("[2슬02-04]".into()), ..base() }), vec![o.id]);
        assert!(found(&conn, &ObservationQuery { student_id: Some(ids[1].clone()), ..base() }).is_empty());
    }

    #[test]
    fn search_excludes_deleted_and_trash_mode_searches_trash_only() {
        let (mut conn, cid, ids, seed_id) = seed();
        let o = add_obs(&mut conn, &cid, &ids[0], 6, "친구와 놀이", &[]);
        delete(&conn, &o.id).unwrap();
        assert_eq!(found(&conn, &q(&cid, "친구")), vec![seed_id.clone()]);
        let trash = ObservationQuery { deleted_only: true, ..q(&cid, "친구") };
        assert_eq!(found(&conn, &trash), vec![o.id.clone()]);
        assert_eq!(count(&conn, &trash).unwrap(), 1);
        assert_eq!(count(&conn, &q(&cid, "친구")).unwrap(), 1);
    }

    #[test]
    fn count_ignores_paging() {
        let (mut conn, cid, ids, _) = seed();
        for day in 6..=9 {
            add_obs(&mut conn, &cid, &ids[0], day, "기록", &[]);
        }
        let query = ObservationQuery { limit: Some(2), before: Some("2027-03-08T10:00:00+09:00".into()), ..q(&cid, "") };
        assert_eq!(list(&conn, &media(), &query).unwrap().len(), 2);
        assert_eq!(count(&conn, &query).unwrap(), 5);
        assert_eq!(count(&conn, &ObservationQuery { text: Some("기록".into()), ..query }).unwrap(), 4);
    }

    #[test]
    fn snippet_uses_chars_and_ellipsis() {
        let t = |s: &str| vec![s.chars().collect::<Vec<char>>()];
        // 앞이 짧으면 앞 말줄임표 없음, 뒤가 길면 뒤 말줄임표
        let long = format!("블록{}", "가".repeat(30));
        let s = snippet_of(&long, &t("블록")).unwrap();
        assert_eq!(s, format!("블록{}…", "가".repeat(20)));
        // 앞뒤 모두 길면 양쪽 말줄임표, 앞 20자 + 낱말 + 뒤 20자
        let mid = format!("{}소방서{}", "나".repeat(25), "다".repeat(25));
        let s = snippet_of(&mid, &t("소방서")).unwrap();
        assert_eq!(s, format!("…{}소방서{}…", "나".repeat(20), "다".repeat(20)));
        // 전체가 짧으면 그대로
        assert_eq!(snippet_of("짧은 글", &t("글")).unwrap(), "짧은 글");
        // 영문 대소문자 무시, 없으면 None
        assert_eq!(snippet_of("Hello World", &t("world")).unwrap(), "Hello World");
        assert!(snippet_of("가나다", &t("라")).is_none());
        // 여러 낱말 중 먼저 나오는 곳
        let both = vec!["다".chars().collect::<Vec<_>>(), "가".chars().collect::<Vec<_>>()];
        assert!(snippet_of("가나다", &both).is_some());
    }

    #[test]
    fn matches_report_each_field_with_snippet() {
        let (mut conn, cid, ids, _) = seed();
        let o = add_obs(&mut conn, &cid, &ids[2], 6, "협력해서 블록 쌓기", &["협력"]);
        let r = list(&conn, &media(), &q(&cid, "협력")).unwrap();
        let m = r.iter().find(|x| x.id == o.id).unwrap().matches.clone().unwrap();
        assert_eq!(m.iter().map(|x| x.field.as_str()).collect::<Vec<_>>(), ["note", "tag"]);
        assert_eq!(m[0].snippet, "협력해서 블록 쌓기");
        assert_eq!(m[1].snippet, "협력");
    }
}
