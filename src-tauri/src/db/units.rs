//! 단원 빌더: 단원·단계·차시, 성취기준 적용 방식, 꼬리 잇기, 집계.
//!
//! DB 로직은 `&Connection`을 받는 순수 함수로 두고, `#[tauri::command]`는 얇게 감싼다.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::observations::{check_standard_code, check_topic_book_id};
use super::{classes, lock, new_id, now_iso, DbExt, DbResult};

const KINDS: [&str; 4] = ["general", "safety", "play", "makingbook"];
const STATUSES: [&str; 3] = ["planned", "done", "skipped"];
const MODES: [&str; 3] = ["repeat", "focus", "emergent"];
const FREE_TOOL: &str = "free";
const MAX_STAGES: usize = 8;
const MAX_TITLE_CHARS: usize = 100;
const MAX_TEXT_CHARS: usize = 10_000;
const MAX_MINUTES: i64 = 600;
const FOLLOW_PREFIX: &str = "앞 차시 학생 질문:\n";

// ───────── 입출력 형식 ─────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitCreateInput {
    pub class_id: String,
    pub topic_book_id: Option<String>,
    pub tool_id: String,
    pub title: String,
    pub stage_titles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SessionStandard {
    pub code: String,
    pub mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    pub unit_id: String,
    pub stage_id: String,
    pub title: String,
    #[serde(default)]
    pub plan: String,
    pub minutes: i64,
    pub kind: String,
    pub scheduled_on: Option<String>,
    pub status: String,
    #[serde(default)]
    pub student_questions: String,
    #[serde(default)]
    pub reflection: String,
    #[serde(default)]
    pub standards: Vec<SessionStandard>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub unit_id: String,
    pub stage_id: String,
    pub position: i64,
    pub title: String,
    pub plan: String,
    pub minutes: i64,
    pub kind: String,
    pub scheduled_on: Option<String>,
    pub status: String,
    pub student_questions: String,
    pub reflection: String,
    pub standards: Vec<SessionStandard>,
    pub follows_session_id: Option<String>,
    pub observation_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct KindMinutes {
    pub general: i64,
    pub safety: i64,
    pub play: i64,
    pub makingbook: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StandardTotal {
    pub code: String,
    pub repeat: i64,
    pub focus: i64,
    pub emergent: i64,
}

/// 건너뜀 차시는 세지 않는다.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub sessions: i64,
    pub done_sessions: i64,
    pub minutes: i64,
    pub done_minutes: i64,
    pub by_kind: KindMinutes,
    pub standards: Vec<StandardTotal>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stage {
    pub id: String,
    pub position: i64,
    pub title: String,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitDetail {
    pub id: String,
    pub class_id: String,
    pub topic_book_id: Option<String>,
    pub tool_id: String,
    pub title: String,
    pub memo: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub stages: Vec<Stage>,
    pub totals: Totals,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitSummary {
    pub id: String,
    pub title: String,
    pub topic_book_id: Option<String>,
    pub tool_id: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub totals: Totals,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextSession {
    pub title: String,
    pub stage_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionOption {
    pub session_id: String,
    pub unit_id: String,
    pub unit_title: String,
    pub topic_book_id: Option<String>,
    pub session_title: String,
    pub scheduled_on: Option<String>,
    pub status: String,
    pub standard_codes: Vec<String>,
}

// ───────── 검사 ─────────

fn clean_title(title: &str, what: &str) -> DbResult<String> {
    let t = title.trim();
    if t.is_empty() {
        return Err(format!("{what} 제목을 입력해 주세요"));
    }
    if t.chars().count() > MAX_TITLE_CHARS {
        return Err(format!("{what} 제목이 너무 깁니다"));
    }
    Ok(t.to_string())
}

fn check_text(text: &str, what: &str) -> DbResult<()> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("{what}이(가) 너무 깁니다"));
    }
    Ok(())
}

fn not_found(what: &str) -> impl Fn(rusqlite::Error) -> String + '_ {
    move |e| match e {
        rusqlite::Error::QueryReturnedNoRows => format!("{what}을(를) 찾을 수 없습니다"),
        e => format!("데이터베이스 오류: {e}"),
    }
}

/// 도구 id는 `free`이거나 소문자·숫자·붙임표로 된 짧은 id. 실제 8종 목록은 교육과정 데이터(프론트)에 있다.
fn check_tool_id(tool: &str) -> DbResult<String> {
    let t = tool.trim();
    let ok = t == FREE_TOOL || (!t.is_empty() && t.len() <= 40 && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    if !ok {
        return Err(format!("단원 개발 도구 id가 올바르지 않습니다: {t}"));
    }
    Ok(t.to_string())
}

fn check_date(date: &str) -> DbResult<String> {
    let d = date.trim();
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| format!("날짜 형식이 올바르지 않습니다: {d}"))?;
    Ok(d.to_string())
}

fn unit_class(conn: &Connection, unit_id: &str) -> DbResult<String> {
    conn.query_row("SELECT class_id FROM units WHERE id = ?1", [unit_id], |r| r.get(0)).map_err(not_found("단원"))
}

fn stage_unit(conn: &Connection, stage_id: &str) -> DbResult<String> {
    conn.query_row("SELECT unit_id FROM unit_stages WHERE id = ?1", [stage_id], |r| r.get(0)).map_err(not_found("단계"))
}

/// 단계가 그 단원의 것인지 확인.
fn check_stage_in_unit(conn: &Connection, stage_id: &str, unit_id: &str) -> DbResult<()> {
    if stage_unit(conn, stage_id)? != unit_id {
        return Err("다른 단원의 단계로는 옮길 수 없습니다".to_string());
    }
    Ok(())
}

fn touch(conn: &Connection, unit_id: &str) -> DbResult<()> {
    conn.execute("UPDATE units SET updated_at = ?2 WHERE id = ?1", params![unit_id, now_iso()]).db()?;
    Ok(())
}

struct CleanSession {
    title: String,
    plan: String,
    minutes: i64,
    kind: String,
    scheduled_on: Option<String>,
    status: String,
    student_questions: String,
    reflection: String,
    standards: Vec<SessionStandard>,
}

fn validate_session(conn: &Connection, input: &SessionInput) -> DbResult<CleanSession> {
    unit_class(conn, &input.unit_id)?;
    check_stage_in_unit(conn, &input.stage_id, &input.unit_id)?;
    let title = clean_title(&input.title, "차시")?;
    for (text, what) in [(&input.plan, "활동 내용"), (&input.student_questions, "학생 질문"), (&input.reflection, "성찰")] {
        check_text(text, what)?;
    }
    if input.minutes <= 0 || input.minutes > MAX_MINUTES {
        return Err(format!("시간은 1분 이상 {MAX_MINUTES}분 이하여야 합니다"));
    }
    if !KINDS.contains(&input.kind.as_str()) {
        return Err(format!("차시 종류가 올바르지 않습니다: {}", input.kind));
    }
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(format!("차시 상태가 올바르지 않습니다: {}", input.status));
    }
    let scheduled_on = match input.scheduled_on.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(d) => Some(check_date(d)?),
    };
    let mut standards: Vec<SessionStandard> = Vec::new();
    for s in &input.standards {
        let code = check_standard_code(&s.code)?;
        if !MODES.contains(&s.mode.as_str()) {
            return Err(format!("성취기준 적용 방식이 올바르지 않습니다: {}", s.mode));
        }
        if !standards.iter().any(|x| x.code == code) {
            standards.push(SessionStandard { code, mode: s.mode.clone() });
        }
    }
    Ok(CleanSession {
        title,
        plan: input.plan.clone(),
        minutes: input.minutes,
        kind: input.kind.clone(),
        scheduled_on,
        status: input.status.clone(),
        student_questions: input.student_questions.clone(),
        reflection: input.reflection.clone(),
        standards,
    })
}

// ───────── 읽기 ─────────

const SESSION_COLS: &str = "s.id, s.unit_id, s.stage_id, s.position, s.title, s.plan, s.minutes, s.kind, s.scheduled_on, s.status,
    s.student_questions, s.reflection, s.follows_session_id,
    (SELECT count(*) FROM observations o WHERE o.unit_session_id = s.id AND o.deleted_at IS NULL),
    s.created_at, s.updated_at";

fn session_row(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        unit_id: r.get(1)?,
        stage_id: r.get(2)?,
        position: r.get(3)?,
        title: r.get(4)?,
        plan: r.get(5)?,
        minutes: r.get(6)?,
        kind: r.get(7)?,
        scheduled_on: r.get(8)?,
        status: r.get(9)?,
        student_questions: r.get(10)?,
        reflection: r.get(11)?,
        standards: vec![],
        follows_session_id: r.get(12)?,
        observation_count: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

/// 단원의 모든 차시(단계 순서, 단계 안 순서).
fn sessions_of_unit(conn: &Connection, unit_id: &str) -> DbResult<Vec<Session>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLS} FROM unit_sessions s JOIN unit_stages g ON g.id = s.stage_id
             WHERE s.unit_id = ?1 ORDER BY g.position, s.position, s.created_at, s.id"
        ))
        .db()?;
    let mut list: Vec<Session> = stmt.query_map([unit_id], session_row).db()?.collect::<Result<_, _>>().db()?;
    let mut stmt = conn
        .prepare(
            "SELECT st.session_id, st.standard_code, st.mode FROM unit_session_standards st
             JOIN unit_sessions s ON s.id = st.session_id WHERE s.unit_id = ?1 ORDER BY st.standard_code",
        )
        .db()?;
    let mut by_session: HashMap<String, Vec<SessionStandard>> = HashMap::new();
    let rows = stmt
        .query_map([unit_id], |r| Ok((r.get::<_, String>(0)?, SessionStandard { code: r.get(1)?, mode: r.get(2)? })))
        .db()?;
    for row in rows {
        let (sid, st) = row.db()?;
        by_session.entry(sid).or_default().push(st);
    }
    for s in &mut list {
        s.standards = by_session.remove(&s.id).unwrap_or_default();
    }
    Ok(list)
}

pub fn compute_totals(sessions: &[Session]) -> Totals {
    let mut t = Totals::default();
    let mut std: BTreeMap<String, StandardTotal> = BTreeMap::new();
    for s in sessions.iter().filter(|s| s.status != "skipped") {
        t.sessions += 1;
        t.minutes += s.minutes;
        if s.status == "done" {
            t.done_sessions += 1;
            t.done_minutes += s.minutes;
        }
        match s.kind.as_str() {
            "safety" => t.by_kind.safety += s.minutes,
            "play" => t.by_kind.play += s.minutes,
            "makingbook" => t.by_kind.makingbook += s.minutes,
            _ => t.by_kind.general += s.minutes,
        }
        for st in &s.standards {
            let e = std.entry(st.code.clone()).or_insert_with(|| StandardTotal { code: st.code.clone(), repeat: 0, focus: 0, emergent: 0 });
            match st.mode.as_str() {
                "repeat" => e.repeat += 1,
                "focus" => e.focus += 1,
                _ => e.emergent += 1,
            }
        }
    }
    t.standards = std.into_values().collect();
    t
}

pub fn get(conn: &Connection, id: &str) -> DbResult<UnitDetail> {
    let mut unit = conn
        .query_row(
            "SELECT id, class_id, topic_book_id, tool_id, title, memo, created_at, updated_at, archived_at FROM units WHERE id = ?1",
            [id],
            |r| {
                Ok(UnitDetail {
                    id: r.get(0)?,
                    class_id: r.get(1)?,
                    topic_book_id: r.get(2)?,
                    tool_id: r.get(3)?,
                    title: r.get(4)?,
                    memo: r.get(5)?,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                    archived_at: r.get(8)?,
                    stages: vec![],
                    totals: Totals::default(),
                })
            },
        )
        .map_err(not_found("단원"))?;
    let mut stmt = conn.prepare("SELECT id, position, title FROM unit_stages WHERE unit_id = ?1 ORDER BY position").db()?;
    let mut stages: Vec<Stage> = stmt
        .query_map([id], |r| Ok(Stage { id: r.get(0)?, position: r.get(1)?, title: r.get(2)?, sessions: vec![] }))
        .db()?
        .collect::<Result<_, _>>()
        .db()?;
    let sessions = sessions_of_unit(conn, id)?;
    unit.totals = compute_totals(&sessions);
    for s in sessions {
        if let Some(stage) = stages.iter_mut().find(|g| g.id == s.stage_id) {
            stage.sessions.push(s);
        }
    }
    unit.stages = stages;
    Ok(unit)
}

pub fn get_session(conn: &Connection, id: &str) -> DbResult<Session> {
    let unit_id: String = conn.query_row("SELECT unit_id FROM unit_sessions WHERE id = ?1", [id], |r| r.get(0)).map_err(not_found("차시"))?;
    sessions_of_unit(conn, &unit_id)?.into_iter().find(|s| s.id == id).ok_or_else(|| "차시를 찾을 수 없습니다".to_string())
}

pub fn list(conn: &Connection, class_id: &str, include_archived: bool) -> DbResult<Vec<UnitSummary>> {
    let mut stmt = conn
        .prepare(
            "SELECT id FROM units WHERE class_id = ?1 AND (?2 OR archived_at IS NULL)
             ORDER BY updated_at DESC, created_at DESC, id",
        )
        .db()?;
    let ids: Vec<String> = stmt.query_map(params![class_id, include_archived], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    ids.iter()
        .map(|id| {
            let u = get(conn, id)?;
            Ok(UnitSummary {
                id: u.id,
                title: u.title,
                topic_book_id: u.topic_book_id,
                tool_id: u.tool_id,
                updated_at: u.updated_at,
                archived_at: u.archived_at,
                totals: u.totals,
            })
        })
        .collect()
}

// ───────── 단원 ─────────

pub fn create(conn: &mut Connection, input: &UnitCreateInput) -> DbResult<UnitDetail> {
    classes::get(conn, &input.class_id)?;
    let tool_id = check_tool_id(&input.tool_id)?;
    let topic = check_topic_book_id(input.topic_book_id.as_deref())?;
    let title = clean_title(&input.title, "단원")?;
    if input.stage_titles.is_empty() || input.stage_titles.len() > MAX_STAGES {
        return Err(format!("단계는 1개에서 {MAX_STAGES}개 사이여야 합니다"));
    }
    let stage_titles = input.stage_titles.iter().map(|t| clean_title(t, "단계")).collect::<DbResult<Vec<_>>>()?;
    let id = new_id();
    let now = now_iso();
    let tx = conn.transaction().db()?;
    tx.execute(
        "INSERT INTO units (id, class_id, topic_book_id, tool_id, title, memo, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, '', ?6, ?6)",
        params![id, input.class_id, topic, tool_id, title, now],
    )
    .db()?;
    for (i, t) in stage_titles.iter().enumerate() {
        tx.execute("INSERT INTO unit_stages (id, unit_id, position, title) VALUES (?1, ?2, ?3, ?4)", params![new_id(), id, i as i64, t]).db()?;
    }
    tx.commit().db()?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, title: &str, memo: &str, topic_book_id: Option<&str>) -> DbResult<UnitDetail> {
    unit_class(conn, id)?;
    let title = clean_title(title, "단원")?;
    check_text(memo, "메모")?;
    let topic = check_topic_book_id(topic_book_id)?;
    conn.execute(
        "UPDATE units SET title = ?2, memo = ?3, topic_book_id = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, title, memo, topic, now_iso()],
    )
    .db()?;
    get(conn, id)
}

pub fn set_archived(conn: &Connection, id: &str, archived: bool) -> DbResult<()> {
    let at = archived.then(now_iso);
    let n = conn.execute("UPDATE units SET archived_at = ?2 WHERE id = ?1", params![id, at]).db()?;
    if n == 0 {
        return Err("단원을 찾을 수 없습니다".to_string());
    }
    Ok(())
}

/// 차시·단계가 함께 지워진다. 연결된 관찰은 남고 `unit_session_id`만 비워진다(외래 키 `ON DELETE SET NULL`).
pub fn delete(conn: &Connection, id: &str) -> DbResult<()> {
    let n = conn.execute("DELETE FROM units WHERE id = ?1", [id]).db()?;
    if n == 0 {
        return Err("단원을 찾을 수 없습니다".to_string());
    }
    Ok(())
}

pub fn rename_stage(conn: &Connection, id: &str, title: &str) -> DbResult<()> {
    let title = clean_title(title, "단계")?;
    let unit_id = stage_unit(conn, id)?;
    conn.execute("UPDATE unit_stages SET title = ?2 WHERE id = ?1", params![id, title]).db()?;
    touch(conn, &unit_id)
}

// ───────── 차시 ─────────

fn stage_session_ids(conn: &Connection, stage_id: &str) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM unit_sessions WHERE stage_id = ?1 ORDER BY position, created_at, id").db()?;
    let ids = stmt.query_map([stage_id], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    Ok(ids)
}

fn write_positions(conn: &Connection, ids: &[String]) -> DbResult<()> {
    for (i, id) in ids.iter().enumerate() {
        conn.execute("UPDATE unit_sessions SET position = ?2 WHERE id = ?1", params![id, i as i64]).db()?;
    }
    Ok(())
}

/// 차시를 `stage_id`의 `position`(범위를 넘으면 끝)에 두고, 영향받은 두 단계의 position을 0..n으로 다시 매긴다.
fn place(conn: &Connection, id: &str, stage_id: &str, position: usize) -> DbResult<()> {
    let old_stage: String = conn.query_row("SELECT stage_id FROM unit_sessions WHERE id = ?1", [id], |r| r.get(0)).map_err(not_found("차시"))?;
    let mut ids = stage_session_ids(conn, stage_id)?;
    ids.retain(|x| x != id);
    ids.insert(position.min(ids.len()), id.to_string());
    conn.execute("UPDATE unit_sessions SET stage_id = ?2 WHERE id = ?1", params![id, stage_id]).db()?;
    write_positions(conn, &ids)?;
    if old_stage != stage_id {
        write_positions(conn, &stage_session_ids(conn, &old_stage)?)?;
    }
    Ok(())
}

fn replace_standards(conn: &Connection, id: &str, standards: &[SessionStandard]) -> DbResult<()> {
    conn.execute("DELETE FROM unit_session_standards WHERE session_id = ?1", [id]).db()?;
    for s in standards {
        conn.execute(
            "INSERT INTO unit_session_standards (session_id, standard_code, mode) VALUES (?1, ?2, ?3)",
            params![id, s.code, s.mode],
        )
        .db()?;
    }
    Ok(())
}

fn insert_session(conn: &Connection, unit_id: &str, stage_id: &str, c: &CleanSession, follows: Option<&str>) -> DbResult<String> {
    let id = new_id();
    let now = now_iso();
    conn.execute(
        "INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, plan, minutes, kind, scheduled_on, status,
                                    student_questions, reflection, follows_session_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, (SELECT count(*) FROM unit_sessions WHERE stage_id = ?3), ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        params![id, unit_id, stage_id, c.title, c.plan, c.minutes, c.kind, c.scheduled_on, c.status, c.student_questions, c.reflection, follows, now],
    )
    .db()?;
    replace_standards(conn, &id, &c.standards)?;
    Ok(id)
}

pub fn create_session(conn: &mut Connection, input: &SessionInput) -> DbResult<Session> {
    let clean = validate_session(conn, input)?;
    let tx = conn.transaction().db()?;
    let id = insert_session(&tx, &input.unit_id, &input.stage_id, &clean, None)?;
    touch(&tx, &input.unit_id)?;
    tx.commit().db()?;
    get_session(conn, &id)
}

pub fn update_session(conn: &mut Connection, id: &str, input: &SessionInput) -> DbResult<Session> {
    let (unit_id, old_stage): (String, String) = conn
        .query_row("SELECT unit_id, stage_id FROM unit_sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(not_found("차시"))?;
    if unit_id != input.unit_id {
        return Err("차시의 단원은 바꿀 수 없습니다".to_string());
    }
    let c = validate_session(conn, input)?;
    let tx = conn.transaction().db()?;
    tx.execute(
        "UPDATE unit_sessions SET title = ?2, plan = ?3, minutes = ?4, kind = ?5, scheduled_on = ?6, status = ?7,
                student_questions = ?8, reflection = ?9, updated_at = ?10 WHERE id = ?1",
        params![id, c.title, c.plan, c.minutes, c.kind, c.scheduled_on, c.status, c.student_questions, c.reflection, now_iso()],
    )
    .db()?;
    replace_standards(&tx, id, &c.standards)?;
    if old_stage != input.stage_id {
        place(&tx, id, &input.stage_id, usize::MAX)?;
    }
    touch(&tx, &unit_id)?;
    tx.commit().db()?;
    get_session(conn, id)
}

pub fn move_session(conn: &mut Connection, id: &str, stage_id: &str, position: i64) -> DbResult<UnitDetail> {
    let unit_id: String = conn.query_row("SELECT unit_id FROM unit_sessions WHERE id = ?1", [id], |r| r.get(0)).map_err(not_found("차시"))?;
    check_stage_in_unit(conn, stage_id, &unit_id)?;
    let tx = conn.transaction().db()?;
    place(&tx, id, stage_id, position.max(0) as usize)?;
    touch(&tx, &unit_id)?;
    tx.commit().db()?;
    get(conn, &unit_id)
}

pub fn delete_session(conn: &mut Connection, id: &str) -> DbResult<()> {
    let (unit_id, stage_id): (String, String) = conn
        .query_row("SELECT unit_id, stage_id FROM unit_sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(not_found("차시"))?;
    let tx = conn.transaction().db()?;
    tx.execute("DELETE FROM unit_sessions WHERE id = ?1", [id]).db()?;
    write_positions(&tx, &stage_session_ids(&tx, &stage_id)?)?;
    touch(&tx, &unit_id)?;
    tx.commit().db()
}

/// 차시를 완료로 바꾸고 학생 질문·성찰을 저장한다. `next`가 있으면 이어지는 새 차시를 같은 트랜잭션에서 만든다.
pub fn complete_and_follow(
    conn: &mut Connection,
    id: &str,
    student_questions: &str,
    reflection: &str,
    next: Option<&NextSession>,
) -> DbResult<UnitDetail> {
    let (unit_id, stage_id, position): (String, String, i64) = conn
        .query_row("SELECT unit_id, stage_id, position FROM unit_sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(not_found("차시"))?;
    check_text(student_questions, "학생 질문")?;
    check_text(reflection, "성찰")?;
    let questions = student_questions.trim();
    let tx = conn.transaction().db()?;
    tx.execute(
        "UPDATE unit_sessions SET status = 'done', student_questions = ?2, reflection = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, questions, reflection, now_iso()],
    )
    .db()?;
    if let Some(next) = next {
        // 잘못된 단계·제목이면 여기서 오류가 나고, 트랜잭션이 버려져 완료 처리도 되돌려진다.
        check_stage_in_unit(&tx, &next.stage_id, &unit_id)?;
        let plan = if questions.is_empty() { String::new() } else { format!("{FOLLOW_PREFIX}{questions}\n\n") };
        let clean = CleanSession {
            title: clean_title(&next.title, "차시")?,
            plan,
            minutes: 40,
            kind: "general".to_string(),
            scheduled_on: None,
            status: "planned".to_string(),
            student_questions: String::new(),
            reflection: String::new(),
            standards: vec![],
        };
        let new_id = insert_session(&tx, &unit_id, &next.stage_id, &clean, Some(id))?;
        let at = if next.stage_id == stage_id { (position + 1) as usize } else { usize::MAX };
        place(&tx, &new_id, &next.stage_id, at)?;
    }
    touch(&tx, &unit_id)?;
    tx.commit().db()?;
    get(conn, &unit_id)
}

// ───────── 관찰 기록 연동 ─────────

/// 그날 날짜가 잡힌 차시와, 단원마다 다음에 할 계획 차시(보관 안 된 단원만).
pub fn options_for_record(conn: &Connection, class_id: &str, date: &str) -> DbResult<Vec<SessionOption>> {
    let date = check_date(date)?;
    let mut stmt = conn
        .prepare("SELECT id FROM units WHERE class_id = ?1 AND archived_at IS NULL ORDER BY updated_at DESC, id")
        .db()?;
    let unit_ids: Vec<String> = stmt.query_map([class_id], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    let mut today: Vec<SessionOption> = Vec::new();
    let mut next: Vec<SessionOption> = Vec::new();
    for uid in unit_ids {
        let unit = get(conn, &uid)?;
        let option = |s: &Session| SessionOption {
            session_id: s.id.clone(),
            unit_id: unit.id.clone(),
            unit_title: unit.title.clone(),
            topic_book_id: unit.topic_book_id.clone(),
            session_title: s.title.clone(),
            scheduled_on: s.scheduled_on.clone(),
            status: s.status.clone(),
            standard_codes: s.standards.iter().map(|x| x.code.clone()).collect(),
        };
        let sessions: Vec<&Session> = unit.stages.iter().flat_map(|g| g.sessions.iter()).collect();
        today.extend(sessions.iter().filter(|s| s.scheduled_on.as_deref() == Some(date.as_str())).map(|s| option(s)));
        if let Some(s) = sessions.iter().find(|s| s.status == "planned") {
            next.push(option(s));
        }
    }
    for o in next {
        if !today.iter().any(|t| t.session_id == o.session_id) {
            today.push(o);
        }
    }
    Ok(today)
}

// ───────── Tauri 명령 ─────────

#[tauri::command]
pub fn unit_list(db: State<Mutex<Connection>>, class_id: String, include_archived: bool) -> DbResult<Vec<UnitSummary>> {
    list(&*lock(&db)?, &class_id, include_archived)
}

#[tauri::command]
pub fn unit_create(db: State<Mutex<Connection>>, input: UnitCreateInput) -> DbResult<UnitDetail> {
    create(&mut *lock(&db)?, &input)
}

#[tauri::command]
pub fn unit_update(db: State<Mutex<Connection>>, id: String, title: String, memo: String, topic_book_id: Option<String>) -> DbResult<UnitDetail> {
    update(&*lock(&db)?, &id, &title, &memo, topic_book_id.as_deref())
}

#[tauri::command]
pub fn unit_archive(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    set_archived(&*lock(&db)?, &id, true)
}

#[tauri::command]
pub fn unit_unarchive(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    set_archived(&*lock(&db)?, &id, false)
}

#[tauri::command]
pub fn unit_delete(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    delete(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn unit_get(db: State<Mutex<Connection>>, id: String) -> DbResult<UnitDetail> {
    get(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn stage_rename(db: State<Mutex<Connection>>, id: String, title: String) -> DbResult<()> {
    rename_stage(&*lock(&db)?, &id, &title)
}

#[tauri::command]
pub fn session_create(db: State<Mutex<Connection>>, input: SessionInput) -> DbResult<Session> {
    create_session(&mut *lock(&db)?, &input)
}

#[tauri::command]
pub fn session_update(db: State<Mutex<Connection>>, id: String, input: SessionInput) -> DbResult<Session> {
    update_session(&mut *lock(&db)?, &id, &input)
}

#[tauri::command]
pub fn session_move(db: State<Mutex<Connection>>, id: String, stage_id: String, position: i64) -> DbResult<UnitDetail> {
    move_session(&mut *lock(&db)?, &id, &stage_id, position)
}

#[tauri::command]
pub fn session_delete(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    delete_session(&mut *lock(&db)?, &id)
}

#[tauri::command]
pub fn session_complete_and_follow(
    db: State<Mutex<Connection>>,
    id: String,
    student_questions: String,
    reflection: String,
    next: Option<NextSession>,
) -> DbResult<UnitDetail> {
    complete_and_follow(&mut *lock(&db)?, &id, &student_questions, &reflection, next.as_ref())
}

#[tauri::command]
pub fn sessions_for_record(db: State<Mutex<Connection>>, class_id: String, date: String) -> DbResult<Vec<SessionOption>> {
    options_for_record(&*lock(&db)?, &class_id, &date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::observations::tests::{input as obs_input, media, seed};
    use crate::db::observations::{self, ObservationQuery};

    fn stages4() -> Vec<String> {
        ["문제 확인", "탐색", "실행", "나눔"].iter().map(|s| s.to_string()).collect()
    }

    fn new_unit(conn: &mut Connection, class: &str) -> UnitDetail {
        create(
            conn,
            &UnitCreateInput {
                class_id: class.to_string(),
                topic_book_id: Some("g1s1-school".into()),
                tool_id: "problem-solving".into(),
                title: "학교 단원".into(),
                stage_titles: stages4(),
            },
        )
        .unwrap()
    }

    fn sin(unit: &UnitDetail, stage: usize, title: &str) -> SessionInput {
        SessionInput {
            unit_id: unit.id.clone(),
            stage_id: unit.stages[stage].id.clone(),
            title: title.to_string(),
            plan: String::new(),
            minutes: 40,
            kind: "general".into(),
            scheduled_on: None,
            status: "planned".into(),
            student_questions: String::new(),
            reflection: String::new(),
            standards: vec![],
        }
    }

    fn titles(u: &UnitDetail, stage: usize) -> Vec<String> {
        u.stages[stage].sessions.iter().map(|s| s.title.clone()).collect()
    }

    fn positions_are_contiguous(u: &UnitDetail) {
        for g in &u.stages {
            let p: Vec<i64> = g.sessions.iter().map(|s| s.position).collect();
            assert_eq!(p, (0..g.sessions.len() as i64).collect::<Vec<_>>(), "{}", g.title);
        }
    }

    #[test]
    fn create_makes_stages_and_free_unit_has_one() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        assert_eq!(u.stages.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["문제 확인", "탐색", "실행", "나눔"]);
        assert_eq!(u.stages.iter().map(|g| g.position).collect::<Vec<_>>(), [0, 1, 2, 3]);
        let free = create(
            &mut conn,
            &UnitCreateInput { class_id: cid.clone(), topic_book_id: None, tool_id: "free".into(), title: "자유".into(), stage_titles: vec!["수업".into()] },
        )
        .unwrap();
        assert_eq!(free.stages.len(), 1);
        assert_eq!(free.topic_book_id, None);
        assert_eq!(list(&conn, &cid, false).unwrap().len(), 2);
    }

    #[test]
    fn create_rejects_bad_input_and_unknown_class() {
        let (mut conn, cid, ..) = seed();
        let base = UnitCreateInput {
            class_id: cid.clone(),
            topic_book_id: None,
            tool_id: "project".into(),
            title: "단원".into(),
            stage_titles: stages4(),
        };
        let try_with = |conn: &mut Connection, f: &dyn Fn(&mut UnitCreateInput)| {
            let mut i = base.clone();
            f(&mut i);
            create(conn, &i)
        };
        assert!(try_with(&mut conn, &|i| i.tool_id = "Bad Tool!".into()).unwrap_err().contains("도구"));
        assert!(try_with(&mut conn, &|i| i.tool_id = "".into()).is_err());
        assert!(try_with(&mut conn, &|i| i.stage_titles = vec![]).unwrap_err().contains("단계"));
        assert!(try_with(&mut conn, &|i| i.stage_titles = vec!["가".into(); 9]).unwrap_err().contains("단계"));
        assert!(try_with(&mut conn, &|i| i.stage_titles = vec!["가".into(); 8]).is_ok());
        assert!(try_with(&mut conn, &|i| i.stage_titles = vec!["  ".into()]).is_err());
        assert!(try_with(&mut conn, &|i| i.topic_book_id = Some("G1S1-School".into())).unwrap_err().contains("주제책"));
        assert!(try_with(&mut conn, &|i| i.title = " ".into()).is_err());
        assert!(try_with(&mut conn, &|i| i.class_id = "nope".into()).unwrap_err().contains("학급"));
        // 다른 학급 목록에는 보이지 않는다.
        let other = crate::db::classes::create(&conn, 2027, 2, "2반").unwrap();
        assert!(list(&conn, &other.id, true).unwrap().is_empty());
    }

    #[test]
    fn session_create_and_update_replace_standards_and_validate() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let mut i = sin(&u, 0, "첫 차시");
        i.standards = vec![
            SessionStandard { code: "[2바01-01]".into(), mode: "repeat".into() },
            SessionStandard { code: "[2슬01-01]".into(), mode: "focus".into() },
            SessionStandard { code: "[2바01-01]".into(), mode: "emergent".into() },
        ];
        let s = create_session(&mut conn, &i).unwrap();
        assert_eq!(s.standards.len(), 2);
        assert_eq!(s.position, 0);
        i.title = "고친 차시".into();
        i.standards = vec![SessionStandard { code: "[2즐01-01]".into(), mode: "emergent".into() }];
        let s2 = update_session(&mut conn, &s.id, &i).unwrap();
        assert_eq!(s2.title, "고친 차시");
        assert_eq!(s2.standards, [SessionStandard { code: "[2즐01-01]".into(), mode: "emergent".into() }]);

        let bad = |f: &dyn Fn(&mut SessionInput)| {
            let mut x = sin(&u, 0, "x");
            f(&mut x);
            x
        };
        for x in [
            bad(&|x| x.standards = vec![SessionStandard { code: "2바01-01".into(), mode: "repeat".into() }]),
            bad(&|x| x.standards = vec![SessionStandard { code: "[2바01-01]".into(), mode: "often".into() }]),
            bad(&|x| x.kind = "party".into()),
            bad(&|x| x.status = "late".into()),
            bad(&|x| x.minutes = 0),
            bad(&|x| x.minutes = -40),
            bad(&|x| x.scheduled_on = Some("3월 5일".into())),
            bad(&|x| x.title = "".into()),
        ] {
            assert!(create_session(&mut conn, &x).is_err());
            assert!(update_session(&mut conn, &s.id, &x).is_err());
        }
    }

    #[test]
    fn session_rejects_stage_of_another_unit() {
        let (mut conn, cid, ..) = seed();
        let a = new_unit(&mut conn, &cid);
        let b = new_unit(&mut conn, &cid);
        let mut i = sin(&a, 0, "a");
        i.stage_id = b.stages[0].id.clone();
        assert!(create_session(&mut conn, &i).unwrap_err().contains("다른 단원"));
        let s = create_session(&mut conn, &sin(&a, 0, "a")).unwrap();
        assert!(update_session(&mut conn, &s.id, &i).unwrap_err().contains("다른 단원"));
        assert!(move_session(&mut conn, &s.id, &b.stages[1].id, 0).unwrap_err().contains("다른 단원"));
        let mut other_unit = sin(&b, 0, "a");
        other_unit.stage_id = b.stages[0].id.clone();
        assert!(update_session(&mut conn, &s.id, &other_unit).is_err());
    }

    #[test]
    fn update_can_change_stage_and_positions_stay_contiguous() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let a = create_session(&mut conn, &sin(&u, 0, "a")).unwrap();
        create_session(&mut conn, &sin(&u, 0, "b")).unwrap();
        create_session(&mut conn, &sin(&u, 1, "c")).unwrap();
        let mut i = sin(&u, 1, "a");
        i.stage_id = u.stages[1].id.clone();
        update_session(&mut conn, &a.id, &i).unwrap();
        let u = get(&conn, &u.id).unwrap();
        assert_eq!(titles(&u, 0), ["b"]);
        assert_eq!(titles(&u, 1), ["c", "a"]);
        positions_are_contiguous(&u);
    }

    #[test]
    fn move_within_and_across_stages_keeps_positions_contiguous() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|t| create_session(&mut conn, &sin(&u, 0, t)).unwrap().id).collect();
        let d = create_session(&mut conn, &sin(&u, 1, "d")).unwrap().id;
        let after = move_session(&mut conn, &ids[2], &u.stages[0].id, 0).unwrap();
        assert_eq!(titles(&after, 0), ["c", "a", "b"]);
        positions_are_contiguous(&after);
        let after = move_session(&mut conn, &ids[2], &u.stages[0].id, 99).unwrap();
        assert_eq!(titles(&after, 0), ["a", "b", "c"]);
        let after = move_session(&mut conn, &ids[0], &u.stages[1].id, 0).unwrap();
        assert_eq!(titles(&after, 0), ["b", "c"]);
        assert_eq!(titles(&after, 1), ["a", "d"]);
        positions_are_contiguous(&after);
        let after = move_session(&mut conn, &d, &u.stages[3].id, 0).unwrap();
        assert_eq!(titles(&after, 1), ["a"]);
        assert_eq!(titles(&after, 3), ["d"]);
        positions_are_contiguous(&after);
        delete_session(&mut conn, &ids[1]).unwrap();
        let after = get(&conn, &u.id).unwrap();
        assert_eq!(titles(&after, 0), ["c"]);
        positions_are_contiguous(&after);
    }

    #[test]
    fn complete_and_follow_in_same_stage_puts_new_session_right_after() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let a = create_session(&mut conn, &sin(&u, 0, "a")).unwrap();
        create_session(&mut conn, &sin(&u, 0, "b")).unwrap();
        let mut with_std = sin(&u, 0, "a");
        with_std.standards = vec![SessionStandard { code: "[2바01-01]".into(), mode: "focus".into() }];
        update_session(&mut conn, &a.id, &with_std).unwrap();
        let next = NextSession { title: "공룡은 어디서 왔을까".into(), stage_id: u.stages[0].id.clone() };
        let after = complete_and_follow(&mut conn, &a.id, "공룡이 궁금해요", "반응이 좋았다", Some(&next)).unwrap();
        assert_eq!(titles(&after, 0), ["a", "공룡은 어디서 왔을까", "b"]);
        positions_are_contiguous(&after);
        let done = &after.stages[0].sessions[0];
        assert_eq!((done.status.as_str(), done.student_questions.as_str(), done.reflection.as_str()), ("done", "공룡이 궁금해요", "반응이 좋았다"));
        let new = &after.stages[0].sessions[1];
        assert_eq!(new.follows_session_id.as_deref(), Some(a.id.as_str()));
        assert!(new.plan.starts_with("앞 차시 학생 질문:\n공룡이 궁금해요\n\n"));
        assert_eq!(new.status, "planned");
        assert!(new.standards.is_empty(), "성취기준은 복사하지 않는다");
    }

    #[test]
    fn complete_and_follow_in_other_stage_appends_and_without_next_only_completes() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let a = create_session(&mut conn, &sin(&u, 0, "a")).unwrap();
        create_session(&mut conn, &sin(&u, 2, "x")).unwrap();
        create_session(&mut conn, &sin(&u, 2, "y")).unwrap();
        let next = NextSession { title: "z".into(), stage_id: u.stages[2].id.clone() };
        let after = complete_and_follow(&mut conn, &a.id, "", "", Some(&next)).unwrap();
        assert_eq!(titles(&after, 2), ["x", "y", "z"]);
        assert!(after.stages[2].sessions[2].plan.is_empty());
        positions_are_contiguous(&after);
        let b = create_session(&mut conn, &sin(&u, 1, "b")).unwrap();
        let after = complete_and_follow(&mut conn, &b.id, "질문", "", None).unwrap();
        assert_eq!(after.totals.sessions, 5);
        assert_eq!(after.totals.done_sessions, 2);
    }

    #[test]
    fn complete_and_follow_is_atomic() {
        let (mut conn, cid, ..) = seed();
        let a = new_unit(&mut conn, &cid);
        let b = new_unit(&mut conn, &cid);
        let s = create_session(&mut conn, &sin(&a, 0, "a")).unwrap();
        let wrong_stage = NextSession { title: "z".into(), stage_id: b.stages[0].id.clone() };
        assert!(complete_and_follow(&mut conn, &s.id, "질문", "성찰", Some(&wrong_stage)).is_err());
        let blank_title = NextSession { title: " ".into(), stage_id: a.stages[0].id.clone() };
        assert!(complete_and_follow(&mut conn, &s.id, "질문", "성찰", Some(&blank_title)).is_err());
        let now = get_session(&conn, &s.id).unwrap();
        assert_eq!((now.status.as_str(), now.student_questions.as_str(), now.reflection.as_str()), ("planned", "", ""));
        assert_eq!(get(&conn, &a.id).unwrap().totals.sessions, 1);
    }

    #[test]
    fn totals_count_minutes_kinds_standards_and_done_separately() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let std = |c: &str, m: &str| SessionStandard { code: c.into(), mode: m.into() };
        let mut a = sin(&u, 0, "a");
        a.standards = vec![std("[2바01-01]", "repeat"), std("[2슬01-01]", "focus")];
        create_session(&mut conn, &a).unwrap();
        let mut b = sin(&u, 1, "b");
        b.minutes = 80;
        b.kind = "safety".into();
        b.status = "done".into();
        b.standards = vec![std("[2바01-01]", "repeat")];
        create_session(&mut conn, &b).unwrap();
        let mut c = sin(&u, 2, "c");
        c.kind = "play".into();
        c.status = "done".into();
        c.standards = vec![std("[2바01-01]", "emergent")];
        create_session(&mut conn, &c).unwrap();
        let mut d = sin(&u, 3, "d");
        d.kind = "makingbook".into();
        d.minutes = 120;
        create_session(&mut conn, &d).unwrap();
        let mut skipped = sin(&u, 3, "e");
        skipped.status = "skipped".into();
        skipped.kind = "play".into();
        create_session(&mut conn, &skipped).unwrap();

        let t = get(&conn, &u.id).unwrap().totals;
        assert_eq!((t.sessions, t.done_sessions, t.minutes, t.done_minutes), (4, 2, 280, 120));
        assert_eq!(t.by_kind, KindMinutes { general: 40, safety: 80, play: 40, makingbook: 120 });
        assert_eq!(
            t.standards,
            [
                StandardTotal { code: "[2바01-01]".into(), repeat: 2, focus: 0, emergent: 1 },
                StandardTotal { code: "[2슬01-01]".into(), repeat: 0, focus: 1, emergent: 0 },
            ]
        );
        assert_eq!(list(&conn, &cid, false).unwrap()[0].totals, t);
    }

    #[test]
    fn deleting_unit_keeps_observation_and_clears_link() {
        let (mut conn, cid, ids, _) = seed();
        let u = new_unit(&mut conn, &cid);
        let s = create_session(&mut conn, &sin(&u, 0, "a")).unwrap();
        let mut inp = obs_input(&cid, &[&ids[0]]);
        inp.unit_session_id = Some(s.id.clone());
        let o = observations::create(&mut conn, &media(), &inp).unwrap();
        assert_eq!(o.unit_session_id.as_deref(), Some(s.id.as_str()));
        assert_eq!(o.unit_session_title.as_deref(), Some("a"));
        assert_eq!(get_session(&conn, &s.id).unwrap().observation_count, 1);
        delete(&conn, &u.id).unwrap();
        let o = observations::get(&conn, &media(), &o.id).unwrap();
        assert_eq!((o.unit_session_id, o.unit_session_title), (None, None));
        let n: i64 = conn.query_row("SELECT count(*) FROM unit_sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert!(delete(&conn, &u.id).is_err());
        let q = ObservationQuery { class_id: cid, ..Default::default() };
        assert_eq!(observations::list(&conn, &media(), &q).unwrap().len(), 2);
    }

    #[test]
    fn deleted_observations_are_not_counted_on_sessions() {
        let (mut conn, cid, ids, _) = seed();
        let u = new_unit(&mut conn, &cid);
        let s = create_session(&mut conn, &sin(&u, 0, "a")).unwrap();
        let mut inp = obs_input(&cid, &[&ids[0]]);
        inp.unit_session_id = Some(s.id.clone());
        let o = observations::create(&mut conn, &media(), &inp).unwrap();
        observations::delete(&conn, &o.id).unwrap();
        assert_eq!(get_session(&conn, &s.id).unwrap().observation_count, 0);
    }

    #[test]
    fn sessions_for_record_lists_today_and_next_planned_and_skips_archived() {
        let (mut conn, cid, ..) = seed();
        let a = new_unit(&mut conn, &cid);
        let mut done = sin(&a, 0, "끝난 차시");
        done.status = "done".into();
        create_session(&mut conn, &done).unwrap();
        let mut today = sin(&a, 1, "오늘 차시");
        today.scheduled_on = Some("2027-03-05".into());
        today.status = "done".into();
        today.standards = vec![SessionStandard { code: "[2즐01-01]".into(), mode: "repeat".into() }];
        create_session(&mut conn, &today).unwrap();
        create_session(&mut conn, &sin(&a, 1, "다음 차시")).unwrap();
        create_session(&mut conn, &sin(&a, 2, "그다음 차시")).unwrap();
        let mut other_day = sin(&a, 2, "다른 날");
        other_day.scheduled_on = Some("2027-03-06".into());
        other_day.status = "done".into();
        create_session(&mut conn, &other_day).unwrap();

        let opts = options_for_record(&conn, &cid, "2027-03-05").unwrap();
        assert_eq!(opts.iter().map(|o| o.session_title.as_str()).collect::<Vec<_>>(), ["오늘 차시", "다음 차시"]);
        assert_eq!(opts[0].standard_codes, ["[2즐01-01]"]);
        assert_eq!(opts[0].unit_title, "학교 단원");
        assert_eq!(opts[0].topic_book_id.as_deref(), Some("g1s1-school"));

        // 다음 계획 차시가 그날 차시이기도 하면 한 번만 나온다.
        let b = new_unit(&mut conn, &cid);
        let mut both = sin(&b, 0, "겹침");
        both.scheduled_on = Some("2027-03-05".into());
        create_session(&mut conn, &both).unwrap();
        let opts = options_for_record(&conn, &cid, "2027-03-05").unwrap();
        assert_eq!(opts.iter().filter(|o| o.session_title == "겹침").count(), 1);
        assert_eq!(opts.len(), 3);

        set_archived(&conn, &a.id, true).unwrap();
        let opts = options_for_record(&conn, &cid, "2027-03-05").unwrap();
        assert_eq!(opts.iter().map(|o| o.session_title.as_str()).collect::<Vec<_>>(), ["겹침"]);
        assert!(options_for_record(&conn, &cid, "03/05").is_err());
        set_archived(&conn, &a.id, false).unwrap();
        assert_eq!(options_for_record(&conn, &cid, "2027-03-05").unwrap().len(), 3);
    }

    #[test]
    fn archive_hides_from_default_list_and_list_is_recent_first() {
        let (mut conn, cid, ..) = seed();
        let a = new_unit(&mut conn, &cid);
        let b = new_unit(&mut conn, &cid);
        conn.execute("UPDATE units SET updated_at = '2027-03-01T00:00:00+09:00' WHERE id = ?1", [&a.id]).unwrap();
        conn.execute("UPDATE units SET updated_at = '2027-03-02T00:00:00+09:00' WHERE id = ?1", [&b.id]).unwrap();
        let l = list(&conn, &cid, false).unwrap();
        assert_eq!(l.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(), [b.id.as_str(), a.id.as_str()]);
        set_archived(&conn, &b.id, true).unwrap();
        assert_eq!(list(&conn, &cid, false).unwrap().len(), 1);
        let all = list(&conn, &cid, true).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|u| u.archived_at.is_some()));
        set_archived(&conn, &b.id, false).unwrap();
        assert_eq!(list(&conn, &cid, false).unwrap().len(), 2);
    }

    #[test]
    fn update_unit_and_rename_stage() {
        let (mut conn, cid, ..) = seed();
        let u = new_unit(&mut conn, &cid);
        let got = update(&conn, &u.id, " 새 제목 ", "메모", None).unwrap();
        assert_eq!((got.title.as_str(), got.memo.as_str(), got.topic_book_id), ("새 제목", "메모", None));
        assert!(update(&conn, &u.id, "", "", None).is_err());
        assert!(update(&conn, &u.id, "x", "", Some("bad")).is_err());
        rename_stage(&conn, &u.stages[1].id, "탐색하기").unwrap();
        assert_eq!(get(&conn, &u.id).unwrap().stages[1].title, "탐색하기");
        assert!(rename_stage(&conn, &u.stages[1].id, " ").is_err());
        assert!(rename_stage(&conn, "nope", "x").is_err());
    }

    #[test]
    fn observation_rejects_session_of_another_class() {
        let (mut conn, cid, ids, _) = seed();
        let other = crate::db::classes::create(&conn, 2027, 2, "2반").unwrap();
        let foreign_unit = new_unit(&mut conn, &other.id);
        let foreign = create_session(&mut conn, &sin(&foreign_unit, 0, "남의 차시")).unwrap();
        let mut inp = obs_input(&cid, &[&ids[0]]);
        inp.unit_session_id = Some(foreign.id.clone());
        assert!(observations::create(&mut conn, &media(), &inp).unwrap_err().contains("차시"));
        inp.unit_session_id = Some("nope".into());
        assert!(observations::create(&mut conn, &media(), &inp).is_err());
        // 수정에서도 같다.
        let (_, _, _, oid) = (0, 0, 0, observations::create(&mut conn, &media(), &obs_input(&cid, &[&ids[0]])).unwrap().id);
        inp.unit_session_id = Some(foreign.id);
        assert!(observations::update(&mut conn, &media(), &oid, &inp).is_err());
        // 자기 학급 차시는 연결되고, 비우면 끊긴다.
        let own_unit = new_unit(&mut conn, &cid);
        let own = create_session(&mut conn, &sin(&own_unit, 0, "내 차시")).unwrap();
        inp.unit_session_id = Some(own.id.clone());
        let o = observations::update(&mut conn, &media(), &oid, &inp).unwrap();
        assert_eq!(o.unit_session_title.as_deref(), Some("내 차시"));
        inp.unit_session_id = None;
        assert_eq!(observations::update(&mut conn, &media(), &oid, &inp).unwrap().unit_session_id, None);
    }
}
