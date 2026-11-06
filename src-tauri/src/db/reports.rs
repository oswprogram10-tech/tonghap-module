//! 학부모 리포트: 아이별 선택 저장, 초안 만들기, 이미지 파일 쓰기.
//!
//! 개인정보 원칙: 이 모듈의 응답에는 **그 아이의 이름만** 들어간다. 함께 기록된 다른 학생은
//! `others_count`(숫자)로만 알려 주고, 메모·자막·차시 제목에 적힌 다른 학생 이름은 "친구"로 바꾼다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Duration, NaiveDate};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::ipc::{InvokeBody, Request};
use tauri::State;

use super::habits::{self, HabitSummary};
use super::observations::{self, check_topic_book_id};
use super::{classes, lock, new_id, now_iso, students, DbExt, DbResult, MediaDir};

pub const MAX_OBSERVATIONS: usize = 4;
pub const MAX_PHOTOS: usize = 4;
pub const MAX_QUOTES: usize = 4;
pub const MAX_LESSONS: usize = 8;
pub const MAX_MESSAGE_CHARS: usize = 400;
pub const MAX_FILE_BYTES: usize = 30 * 1024 * 1024;
const MAX_PERIOD_DAYS: i64 = 366;
const FRIEND: &str = "친구";
const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportStudent {
    pub id: String,
    pub number: i64,
    pub name: String,
}

/// 리포트에 넣을 관찰. 학생은 이 아이 한 명뿐이고 나머지는 숫자로만 준다.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportObservation {
    pub id: String,
    pub observed_at: String,
    /// 다른 학생 이름을 "친구"로 바꾼 메모.
    pub note: String,
    pub standard_codes: Vec<String>,
    pub tags: Vec<String>,
    pub others_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportPhoto {
    pub id: String,
    pub abs_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportAudio {
    pub id: String,
    /// 다른 학생 이름을 "친구"로 바꾼 자막.
    pub transcript: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub observation: ReportObservation,
    /// 이 아이 한 명만 기록된 관찰인가(사진을 기본 포함하는 조건).
    pub solo_for_student: bool,
    pub photos: Vec<ReportPhoto>,
    pub audio_with_transcript: Vec<ReportAudio>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportDraft {
    /// 저장된 리포트의 id. 아직 저장 전이면 `None`.
    pub id: Option<String>,
    pub exported_at: Option<String>,
    pub school_year: i64,
    pub grade: i64,
    pub class_name: String,
    pub student: ReportStudent,
    pub topic_book_id: Option<String>,
    pub from: String,
    pub to: String,
    pub lessons: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub observation_ids: Vec<String>,
    pub attachment_ids: Vec<String>,
    pub quote_attachment_ids: Vec<String>,
    pub habits: Vec<HabitSummary>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportInput {
    pub student_id: String,
    pub topic_book_id: Option<String>,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub observation_ids: Vec<String>,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    #[serde(default)]
    pub quote_attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Period {
    pub from: String,
    pub to: String,
}

pub(crate) fn parse_date(s: &str) -> DbResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "날짜 형식이 올바르지 않습니다".to_string())
}

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// 기간을 검사하고 (시작, 끝, 일수)를 돌려준다.
pub(crate) fn check_period(from: &str, to: &str) -> DbResult<(String, String, i64)> {
    let (f, t) = (parse_date(from)?, parse_date(to)?);
    if f > t {
        return Err("시작일이 끝일보다 늦습니다".to_string());
    }
    let days = (t - f).num_days() + 1;
    if days > MAX_PERIOD_DAYS {
        return Err("기간은 1년(366일) 이하로 정해 주세요".to_string());
    }
    Ok((fmt(f), fmt(t), days))
}

/// 학급의 다른 학생 이름(전체 이름, 성을 뗀 이름)을 "친구"로 바꾼다. 이 아이 이름은 지키고,
/// 한 글자 이름은 일반 낱말을 망가뜨리므로 바꾸지 않는다.
pub(crate) struct Scrubber {
    own: Vec<String>,
    others: Vec<String>,
}

impl Scrubber {
    pub(crate) fn new(conn: &Connection, class_id: &str, student_id: &str) -> DbResult<Self> {
        let all = students::list(conn, class_id, true)?;
        let mut own = Vec::new();
        let mut others: Vec<String> = Vec::new();
        for s in all.iter().filter(|s| s.id == student_id) {
            let name = s.name.trim().to_string();
            if name.chars().count() >= 3 {
                own.push(name.chars().skip(1).collect());
            }
            own.push(name);
        }
        for s in all.iter().filter(|s| s.id != student_id) {
            let name = s.name.trim().to_string();
            if name.chars().count() < 2 || own.contains(&name) {
                continue;
            }
            if name.chars().count() >= 3 {
                let given: String = name.chars().skip(1).collect();
                if !own.iter().any(|o| o.contains(&given) || given.contains(o.as_str())) {
                    others.push(given);
                }
            }
            others.push(name);
        }
        others.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
        others.dedup();
        own.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
        Ok(Scrubber { own, others })
    }

    /// 다른 학생 이름을 "친구"로 바꾸고 이 아이 이름은 그대로 둔다.
    pub(crate) fn scrub(&self, text: &str) -> String {
        self.scrub_inner(text, None)
    }

    /// 다른 학생 이름은 "친구", 이 아이 이름은 `alias`로 바꾼다(평어 근거 묶음용).
    pub(crate) fn scrub_own_to(&self, text: &str, alias: &str) -> String {
        self.scrub_inner(text, Some(alias))
    }

    /// 학급의 다른 학생 이름(전체 이름과 성을 뗀 이름).
    pub(crate) fn other_names(&self) -> &[String] {
        &self.others
    }

    fn scrub_inner(&self, text: &str, own_to: Option<&str>) -> String {
        // 이 아이 이름이 다른 이름의 일부여도 망가지지 않도록 먼저 자리표시자로 바꿔 둔다.
        let mut out = text.to_string();
        for (i, o) in self.own.iter().enumerate() {
            if o.chars().count() >= 2 {
                out = out.replace(o.as_str(), &format!("\u{1}{i}\u{1}"));
            }
        }
        for n in &self.others {
            out = replace_name(out, n, FRIEND);
        }
        for (i, o) in self.own.iter().enumerate() {
            let placeholder = format!("\u{1}{i}\u{1}");
            out = match own_to {
                Some(alias) => replace_name(out, &placeholder, alias),
                None => out.replace(&placeholder, o),
            };
        }
        out
    }
}

/// 이름을 `to`로 바꾼다. "서준이가"처럼 이름 뒤에 붙은 호칭 "이"도 함께 지운다.
fn replace_name(mut out: String, name: &str, to: &str) -> String {
    for suffix in ["이가", "이는", "이도", "이를", "이랑", "이와", "이한테", "이에게", "이의"] {
        out = out.replace(&format!("{name}{suffix}"), &format!("{to}{}", &suffix["이".len()..]));
    }
    out.replace(name, to)
}

fn candidate(o: &observations::Observation, student_id: &str, sc: &Scrubber) -> Candidate {
    let others_count = o.students.iter().filter(|s| s.id != student_id).count();
    let photos = o
        .attachments
        .iter()
        // HEIC는 화면에서 그릴 수 없어 리포트에 넣지 않는다.
        .filter(|a| a.kind == "photo" && a.mime != "image/heic")
        .map(|a| ReportPhoto { id: a.id.clone(), abs_path: a.abs_path.clone() })
        .collect();
    let audio = o
        .attachments
        .iter()
        .filter(|a| a.kind == "audio")
        .filter_map(|a| {
            let t = a.transcript.as_deref()?.trim();
            (a.transcript_status == "done" && !t.is_empty()).then(|| ReportAudio { id: a.id.clone(), transcript: sc.scrub(t) })
        })
        .collect();
    Candidate {
        observation: ReportObservation {
            id: o.id.clone(),
            observed_at: o.observed_at.clone(),
            note: sc.scrub(&o.note),
            standard_codes: o.standard_codes.clone(),
            tags: o.tags.iter().map(|t| sc.scrub(t)).collect(),
            others_count,
        },
        solo_for_student: others_count == 0,
        photos,
        audio_with_transcript: audio,
    }
}

fn candidates(conn: &Connection, media: &Path, class_id: &str, student_id: &str, from: &str, to: &str, sc: &Scrubber) -> DbResult<Vec<Candidate>> {
    let mut stmt = conn
        .prepare(
            "SELECT o.id FROM observations o
             JOIN observation_students os ON os.observation_id = o.id AND os.student_id = ?1
             WHERE o.class_id = ?2 AND o.deleted_at IS NULL AND substr(o.observed_at, 1, 10) BETWEEN ?3 AND ?4
             ORDER BY o.observed_at DESC, o.created_at DESC, o.id",
        )
        .db()?;
    let ids: Vec<String> = stmt.query_map(params![student_id, class_id, from, to], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    ids.iter().map(|id| Ok(candidate(&observations::get(conn, media, id)?, student_id, sc))).collect()
}

/// 그 기간·주제책 단원의 완료된 차시 제목(최대 8개, 날짜순).
fn lessons(conn: &Connection, class_id: &str, topic: Option<&str>, from: &str, to: &str, sc: &Scrubber) -> DbResult<Vec<String>> {
    let Some(topic) = topic else { return Ok(vec![]) };
    let mut stmt = conn
        .prepare(
            "SELECT s.title FROM unit_sessions s
             JOIN units u ON u.id = s.unit_id
             JOIN unit_stages g ON g.id = s.stage_id
             WHERE u.class_id = ?1 AND u.topic_book_id = ?2 AND s.status = 'done'
               AND COALESCE(s.scheduled_on, substr(s.updated_at, 1, 10)) BETWEEN ?3 AND ?4
             ORDER BY COALESCE(s.scheduled_on, substr(s.updated_at, 1, 10)), u.created_at, g.position, s.position
             LIMIT ?5",
        )
        .db()?;
    let rows = stmt.query_map(params![class_id, topic, from, to, MAX_LESSONS as i64], |r| r.get::<_, String>(0)).db()?;
    rows.map(|r| r.map(|t| sc.scrub(&t)).db()).collect()
}

/// 기본 선택: 메모 있는 관찰 중 성취기준이 붙은 것 우선, 그다음 최신 4개 / 단독 관찰의 사진 최신 4장.
fn default_selection(cands: &[Candidate]) -> (Vec<String>, Vec<String>) {
    let mut with_note: Vec<&Candidate> = cands.iter().filter(|c| !c.observation.note.trim().is_empty()).collect();
    // 후보는 이미 최신순이고 이 정렬은 안정적이다.
    with_note.sort_by_key(|c| c.observation.standard_codes.is_empty());
    let mut chosen: Vec<&Candidate> = with_note.into_iter().take(MAX_OBSERVATIONS).collect();
    chosen.sort_by(|a, b| b.observation.observed_at.cmp(&a.observation.observed_at));
    let obs = chosen.iter().map(|c| c.observation.id.clone()).collect();
    let photos = cands.iter().filter(|c| c.solo_for_student).flat_map(|c| c.photos.iter()).take(MAX_PHOTOS).map(|p| p.id.clone()).collect();
    (obs, photos)
}

struct Stored {
    id: String,
    message: String,
    observation_ids: Vec<String>,
    attachment_ids: Vec<String>,
    quote_attachment_ids: Vec<String>,
    exported_at: Option<String>,
}

fn load_stored(conn: &Connection, student_id: &str, topic: Option<&str>, from: &str, to: &str) -> DbResult<Option<Stored>> {
    let row = conn.query_row(
        "SELECT id, message, observation_ids, attachment_ids, quote_attachment_ids, exported_at FROM parent_reports
         WHERE student_id = ?1 AND topic_book_id IS ?2 AND period_from = ?3 AND period_to = ?4",
        params![student_id, topic, from, to],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get(5)?)),
    );
    let (id, message, o, a, q, exported_at) = match row {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(format!("데이터베이스 오류: {e}")),
    };
    let list = |s: &str| serde_json::from_str::<Vec<String>>(s).unwrap_or_default();
    Ok(Some(Stored { id, message, observation_ids: list(&o), attachment_ids: list(&a), quote_attachment_ids: list(&q), exported_at }))
}

pub fn draft(conn: &Connection, media: &Path, student_id: &str, topic: Option<&str>, from: &str, to: &str) -> DbResult<ReportDraft> {
    let (from, to, days) = check_period(from, to)?;
    let topic = check_topic_book_id(topic)?;
    let student = students::get(conn, student_id)?;
    let class = classes::get(conn, &student.class_id)?;
    let sc = Scrubber::new(conn, &class.id, student_id)?;
    let cands = candidates(conn, media, &class.id, student_id, &from, &to, &sc)?;
    let lessons = lessons(conn, &class.id, topic.as_deref(), &from, &to, &sc)?;
    let habits = habits::student_summary(conn, student_id, &to, days)?;

    let (id, exported_at, message, obs, att, quotes) = match load_stored(conn, student_id, topic.as_deref(), &from, &to)? {
        Some(s) => {
            // 그 사이 지워졌거나 기간에서 빠진 항목은 선택에서 뺀다.
            let obs_ok: HashSet<&str> = cands.iter().map(|c| c.observation.id.as_str()).collect();
            let photo_ok: HashSet<&str> = cands.iter().flat_map(|c| c.photos.iter().map(|p| p.id.as_str())).collect();
            let audio_ok: HashSet<&str> = cands.iter().flat_map(|c| c.audio_with_transcript.iter().map(|p| p.id.as_str())).collect();
            let keep = |ids: Vec<String>, ok: &HashSet<&str>| ids.into_iter().filter(|i| ok.contains(i.as_str())).collect::<Vec<_>>();
            (
                Some(s.id),
                s.exported_at,
                s.message,
                keep(s.observation_ids, &obs_ok),
                keep(s.attachment_ids, &photo_ok),
                keep(s.quote_attachment_ids, &audio_ok),
            )
        }
        None => {
            let (o, p) = default_selection(&cands);
            (None, None, String::new(), o, p, vec![])
        }
    };
    Ok(ReportDraft {
        id,
        exported_at,
        school_year: class.school_year,
        grade: class.grade,
        class_name: class.name,
        student: ReportStudent { id: student.id, number: student.number, name: student.name },
        topic_book_id: topic,
        from,
        to,
        lessons,
        candidates: cands,
        observation_ids: obs,
        attachment_ids: att,
        quote_attachment_ids: quotes,
        habits,
        message,
    })
}

fn dedup(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in ids {
        if !out.contains(i) {
            out.push(i.clone());
        }
    }
    out
}

pub fn save(conn: &Connection, media: &Path, input: &ReportInput) -> DbResult<ReportDraft> {
    let (from, to, _) = check_period(&input.from, &input.to)?;
    let topic = check_topic_book_id(input.topic_book_id.as_deref())?;
    let student = students::get(conn, &input.student_id)?;
    let message = input.message.trim().to_string();
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!("선생님이 전하는 말은 {MAX_MESSAGE_CHARS}자까지 쓸 수 있습니다"));
    }
    let (obs, att, quotes) = (dedup(&input.observation_ids), dedup(&input.attachment_ids), dedup(&input.quote_attachment_ids));
    if obs.len() > MAX_OBSERVATIONS {
        return Err(format!("관찰은 {MAX_OBSERVATIONS}개까지 고를 수 있습니다"));
    }
    if att.len() > MAX_PHOTOS {
        return Err(format!("사진은 {MAX_PHOTOS}장까지 고를 수 있습니다"));
    }
    if quotes.len() > MAX_QUOTES {
        return Err(format!("자막 인용은 {MAX_QUOTES}개까지 고를 수 있습니다"));
    }
    // 그 아이의 기간 안 관찰에 딸린 것만 고를 수 있다(다른 학생의 기록·사진은 거부).
    let sc = Scrubber::new(conn, &student.class_id, &student.id)?;
    let cands = candidates(conn, media, &student.class_id, &student.id, &from, &to, &sc)?;
    let obs_ok: HashSet<&str> = cands.iter().map(|c| c.observation.id.as_str()).collect();
    let photo_ok: HashSet<&str> = cands.iter().flat_map(|c| c.photos.iter().map(|p| p.id.as_str())).collect();
    let audio_ok: HashSet<&str> = cands.iter().flat_map(|c| c.audio_with_transcript.iter().map(|p| p.id.as_str())).collect();
    if obs.iter().any(|i| !obs_ok.contains(i.as_str())) {
        return Err("이 학생의 기간 안 관찰만 고를 수 있습니다".to_string());
    }
    if att.iter().any(|i| !photo_ok.contains(i.as_str())) {
        return Err("이 학생의 기간 안 관찰 사진만 고를 수 있습니다".to_string());
    }
    if quotes.iter().any(|i| !audio_ok.contains(i.as_str())) {
        return Err("자막이 있는 이 학생의 음성 메모만 인용할 수 있습니다".to_string());
    }
    let json = |v: &Vec<String>| serde_json::to_string(v).expect("JSON");
    let now = now_iso();
    match load_stored(conn, &student.id, topic.as_deref(), &from, &to)? {
        Some(s) => {
            conn.execute(
                "UPDATE parent_reports SET message = ?2, observation_ids = ?3, attachment_ids = ?4, quote_attachment_ids = ?5, updated_at = ?6 WHERE id = ?1",
                params![s.id, message, json(&obs), json(&att), json(&quotes), now],
            )
            .db()?;
        }
        None => {
            conn.execute(
                "INSERT INTO parent_reports (id, class_id, student_id, topic_book_id, period_from, period_to, message,
                   observation_ids, attachment_ids, quote_attachment_ids, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![new_id(), student.class_id, student.id, topic, from, to, message, json(&obs), json(&att), json(&quotes), now],
            )
            .db()?;
        }
    }
    draft(conn, media, &student.id, topic.as_deref(), &from, &to)
}

pub fn mark_exported(conn: &Connection, id: &str) -> DbResult<()> {
    let n = conn.execute("UPDATE parent_reports SET exported_at = ?2 WHERE id = ?1", params![id, now_iso()]).db()?;
    if n == 0 {
        return Err("리포트를 찾을 수 없습니다. 먼저 저장해 주세요".to_string());
    }
    Ok(())
}

/// 기본 기간: 그 주제책 단원의 첫 차시 날짜 ~ 마지막 완료 차시 날짜. 없으면 오늘까지 최근 6주.
pub fn default_period(conn: &Connection, class_id: &str, topic: Option<&str>, today: NaiveDate) -> DbResult<Period> {
    let topic = check_topic_book_id(topic)?;
    let fallback = Period { from: fmt(today - Duration::days(41)), to: fmt(today) };
    let Some(topic) = topic else { return Ok(fallback) };
    let (first, last): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT min(s.scheduled_on), max(CASE WHEN s.status = 'done' THEN s.scheduled_on END)
             FROM unit_sessions s JOIN units u ON u.id = s.unit_id
             WHERE u.class_id = ?1 AND u.topic_book_id = ?2 AND s.status != 'skipped' AND s.scheduled_on IS NOT NULL",
            params![class_id, topic],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .db()?;
    let (Some(first), Some(last)) = (first, last) else { return Ok(fallback) };
    let (f, l) = (parse_date(&first)?, parse_date(&last)?);
    if f > l {
        return Ok(fallback);
    }
    let f = f.max(l - Duration::days(MAX_PERIOD_DAYS - 1));
    Ok(Period { from: fmt(f), to: fmt(l) })
}

/// `x-path` 헤더(UTF-8을 percent-encoding한 값)를 풀어 쓴다.
fn percent_decode(s: &str) -> DbResult<String> {
    let bad = || "저장 경로가 올바르지 않습니다".to_string();
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or_else(bad)?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| bad())?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| bad())
}

/// 교사가 고른 경로에 PNG를 쓴다. `.part`에 쓴 뒤 이름을 바꿔 기존 파일을 덮어쓴다.
pub fn write_file(path: &str, data: &[u8]) -> DbResult<()> {
    let target = PathBuf::from(path);
    let ext = target.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if ext != "png" {
        return Err("리포트는 .png 파일로만 저장할 수 있습니다".to_string());
    }
    if !target.is_absolute() {
        return Err("저장 경로가 올바르지 않습니다".to_string());
    }
    if data.is_empty() || !data.starts_with(PNG_MAGIC) {
        return Err("PNG 이미지가 아닙니다".to_string());
    }
    if data.len() > MAX_FILE_BYTES {
        return Err("이미지가 너무 큽니다(30MB 이하만 저장할 수 있습니다)".to_string());
    }
    let dir = target.parent().filter(|d| d.is_dir()).ok_or("저장할 폴더가 없습니다")?;
    if target.is_dir() {
        return Err("같은 이름의 폴더가 있어 저장할 수 없습니다".to_string());
    }
    let name = target.file_name().and_then(|n| n.to_str()).ok_or("저장 경로가 올바르지 않습니다")?;
    let part = dir.join(format!("{name}.part"));
    std::fs::write(&part, data).map_err(|e| format!("파일을 저장하지 못했습니다: {e}"))?;
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("파일을 저장하지 못했습니다: {e}")
    })
}

#[tauri::command]
pub fn report_draft(
    db: State<Mutex<Connection>>,
    media: State<MediaDir>,
    student_id: String,
    topic_book_id: Option<String>,
    from: String,
    to: String,
) -> DbResult<ReportDraft> {
    draft(&*lock(&db)?, &media.0, &student_id, topic_book_id.as_deref(), &from, &to)
}

#[tauri::command]
pub fn report_save(db: State<Mutex<Connection>>, media: State<MediaDir>, input: ReportInput) -> DbResult<ReportDraft> {
    save(&*lock(&db)?, &media.0, &input)
}

#[tauri::command]
pub fn report_mark_exported(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    mark_exported(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn report_default_period(db: State<Mutex<Connection>>, class_id: String, topic_book_id: Option<String>) -> DbResult<Period> {
    default_period(&*lock(&db)?, &class_id, topic_book_id.as_deref(), chrono::Local::now().date_naive())
}

/// PNG를 JSON 배열로 보내지 않도록 원시 바이트 본문으로 받는다.
#[tauri::command]
pub fn report_write_file(request: Request<'_>) -> DbResult<()> {
    let InvokeBody::Raw(data) = request.body() else {
        return Err("원시 바이트 본문이 필요합니다".to_string());
    };
    let raw = request.headers().get("x-path").and_then(|v| v.to_str().ok()).ok_or("요청 헤더 x-path이(가) 없습니다")?;
    write_file(&percent_decode(raw)?, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::observations::{create, ObservationInput};
    use crate::db::{attachments, test_conn};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-report-{}-{}", name, new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    struct Ctx {
        conn: Connection,
        media: PathBuf,
        class: String,
        /// 김하늘, 박서준, 이지우
        ids: Vec<String>,
    }

    fn seed() -> Ctx {
        let mut conn = test_conn();
        let c = classes::create(&conn, 2027, 1, "1반").unwrap();
        students::import(&mut conn, &c.id, "1 김하늘\n2 박서준\n3 이지우").unwrap();
        let ids = students::list(&conn, &c.id, true).unwrap().into_iter().map(|s| s.id).collect();
        Ctx { conn, media: tmp("media"), class: c.id, ids }
    }

    fn obs(cx: &mut Ctx, students: &[usize], day: &str, note: &str, codes: &[&str]) -> observations::Observation {
        let input = ObservationInput {
            class_id: cx.class.clone(),
            observed_at: format!("{day}T10:00:00+09:00"),
            topic_book_id: Some("g1s1-school".into()),
            note: note.into(),
            student_ids: students.iter().map(|i| cx.ids[*i].clone()).collect(),
            standard_codes: codes.iter().map(|c| c.to_string()).collect(),
            tags: vec!["협력".into()],
            unit_session_id: None,
        };
        create(&mut cx.conn, &cx.media, &input).unwrap()
    }

    fn photo(cx: &Ctx, o: &str) -> String {
        let p = tmp("src").join("a.png");
        std::fs::write(&p, b"img").unwrap();
        attachments::add_file(&cx.conn, &cx.media, &crate::db::media::test_key(), o, p.to_str().unwrap()).unwrap().id
    }

    const FROM: &str = "2027-03-01";
    const TO: &str = "2027-04-30";

    fn draft_of(cx: &Ctx, i: usize) -> ReportDraft {
        draft(&cx.conn, &cx.media, &cx.ids[i], Some("g1s1-school"), FROM, TO).unwrap()
    }

    #[test]
    fn default_selection_prefers_standards_then_newest_and_only_solo_photos() {
        let mut cx = seed();
        // 메모 있는 6개: 0번과 2번에만 성취기준
        let days = ["03-02", "03-03", "03-04", "03-05", "03-06", "03-07"];
        let mut made = vec![];
        for (i, d) in days.iter().enumerate() {
            let codes: &[&str] = if i == 0 || i == 2 { &["[2바01-01]"] } else { &[] };
            made.push(obs(&mut cx, &[0], &format!("2027-{d}"), &format!("메모{i}"), codes));
        }
        // 메모 없는 관찰, 기간 밖, 삭제된 관찰, 다른 학생만의 관찰은 후보에서 빠지거나 선택되지 않는다.
        obs(&mut cx, &[0], "2027-04-01", "", &["[2바01-01]"]);
        obs(&mut cx, &[0], "2027-05-02", "기간 밖", &["[2바01-01]"]);
        let gone = obs(&mut cx, &[0], "2027-03-20", "삭제됨", &["[2바01-01]"]);
        observations::delete(&cx.conn, &gone.id).unwrap();
        obs(&mut cx, &[1], "2027-03-21", "서준 혼자", &["[2바01-01]"]);

        let d = draft_of(&cx, 0);
        assert_eq!(d.candidates.len(), 7, "기간 안·삭제 안 된·그 아이 관찰만");
        assert!(d.candidates.windows(2).all(|w| w[0].observation.observed_at >= w[1].observation.observed_at), "최신순");
        // 성취기준 붙은 둘(0, 2) + 나머지 중 최신 둘(5, 4), 최신순 표시
        let want: Vec<&str> = [5, 4, 2, 0].iter().map(|i| made[*i].id.as_str()).collect();
        assert_eq!(d.observation_ids.iter().map(String::as_str).collect::<Vec<_>>(), want);
        assert!(d.id.is_none() && d.message.is_empty() && d.quote_attachment_ids.is_empty());

        // 사진: 단독 관찰 사진 최신 4장만, 함께 기록된 관찰 사진은 제외
        let group = obs(&mut cx, &[0, 1], "2027-04-10", "함께", &[]);
        let group_photo = photo(&cx, &group.id);
        let solo_photos: Vec<String> = made.iter().map(|o| photo(&cx, &o.id)).collect();
        let d = draft_of(&cx, 0);
        assert_eq!(d.attachment_ids, [5, 4, 3, 2].iter().map(|i| solo_photos[*i].clone()).collect::<Vec<_>>());
        assert!(!d.attachment_ids.contains(&group_photo));
        let g = d.candidates.iter().find(|c| c.observation.id == group.id).unwrap();
        assert!(!g.solo_for_student);
        assert_eq!((g.observation.others_count, g.photos.len()), (1, 1), "후보로는 보이고 교사가 켤 수 있다");
    }

    #[test]
    fn no_other_student_name_appears_anywhere_in_the_serialized_response() {
        let mut cx = seed();
        let a = obs(&mut cx, &[0, 1], "2027-03-05", "박서준이랑 같이 블록을 쌓았다. 서준이가 먼저 도왔고 지우도 구경했다. 이지우는 웃었다. 김하늘은 기뻐했다.", &["[2바01-01]"]);
        let au = attachments::add_bytes(&cx.conn, &cx.media, &crate::db::media::test_key(), &a.id, "audio", "audio/webm", b"x").unwrap();
        attachments::set_transcript(&cx.conn, &cx.media, &au.id, "서준이가 박서준 이름을 불렀어요. 이지우 하늘이").unwrap();
        obs(&mut cx, &[0, 1, 2], "2027-03-06", "셋이 박서준 이지우와 함께", &[]);
        // 단원 차시 제목에도 이름이 있을 수 있다.
        cx.conn
            .execute_batch(&format!(
                "INSERT INTO units (id, class_id, topic_book_id, tool_id, title, created_at, updated_at) VALUES ('u', '{c}', 'g1s1-school', 'free', '단원', 't', 't');
                 INSERT INTO unit_stages (id, unit_id, position, title) VALUES ('g', 'u', 0, '단계');
                 INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, status, scheduled_on, created_at, updated_at)
                   VALUES ('s', 'u', 'g', 0, '박서준 발표 듣기', 'done', '2027-03-08', 't', 't');",
                c = cx.class
            ))
            .unwrap();

        let d0 = draft_of(&cx, 0);
        let input = ReportInput {
            student_id: cx.ids[0].clone(),
            topic_book_id: Some("g1s1-school".into()),
            from: FROM.into(),
            to: TO.into(),
            message: "잘 지냈어요".into(),
            observation_ids: d0.observation_ids.clone(),
            attachment_ids: vec![],
            quote_attachment_ids: vec![au.id.clone()],
        };
        let saved = save(&cx.conn, &cx.media, &input).unwrap();
        for d in [d0, draft_of(&cx, 0), saved] {
            let json = serde_json::to_string(&d).unwrap();
            for other in ["박서준", "서준", "이지우", "지우"] {
                assert!(!json.contains(other), "{other} 이(가) 응답에 있음: {json}");
            }
            assert!(json.contains("김하늘"), "이 아이 이름은 남는다");
            assert!(json.contains("친구"));
            assert_eq!(d.lessons, ["친구 발표 듣기"]);
        }
        let d = draft_of(&cx, 0);
        let first = d.candidates.iter().find(|c| c.observation.id == a.id).unwrap();
        assert_eq!(first.observation.others_count, 1);
        assert_eq!(first.audio_with_transcript[0].transcript, "친구가 친구 이름을 불렀어요. 친구 하늘이");
        assert_eq!(first.observation.note, "친구랑 같이 블록을 쌓았다. 친구가 먼저 도왔고 친구도 구경했다. 친구는 웃었다. 김하늘은 기뻐했다.");
    }

    #[test]
    fn save_roundtrip_keeps_order_and_rejects_foreign_or_excess_selection() {
        let mut cx = seed();
        let a = obs(&mut cx, &[0], "2027-03-02", "가", &[]);
        let b = obs(&mut cx, &[0], "2027-03-03", "나", &[]);
        let c = obs(&mut cx, &[0], "2027-03-04", "다", &[]);
        let other = obs(&mut cx, &[1], "2027-03-05", "서준 기록", &[]);
        let other_photo = photo(&cx, &other.id);
        let mine = photo(&cx, &a.id);
        let ids = cx.ids.clone();
        let input = move |obs: Vec<&str>, att: Vec<&str>| ReportInput {
            student_id: ids[0].clone(),
            topic_book_id: Some("g1s1-school".into()),
            from: FROM.into(),
            to: TO.into(),
            message: "  한 학기 잘 지냈어요  ".into(),
            observation_ids: obs.into_iter().map(String::from).collect(),
            attachment_ids: att.into_iter().map(String::from).collect(),
            quote_attachment_ids: vec![],
        };
        let saved = save(&cx.conn, &cx.media, &input(vec![&c.id, &a.id, &b.id], vec![&mine])).unwrap();
        assert!(saved.id.is_some());
        assert_eq!(saved.observation_ids, [c.id.clone(), a.id.clone(), b.id.clone()]);
        assert_eq!(saved.message, "한 학기 잘 지냈어요");
        let again = draft_of(&cx, 0);
        assert_eq!((again.id.clone(), again.observation_ids.clone(), again.attachment_ids.clone()), (saved.id.clone(), saved.observation_ids.clone(), vec![mine.clone()]));

        // 다시 저장하면 같은 행을 고친다.
        let second = save(&cx.conn, &cx.media, &input(vec![&a.id], vec![])).unwrap();
        assert_eq!(second.id, saved.id);
        let count = |cx: &Ctx| -> i64 { cx.conn.query_row("SELECT count(*) FROM parent_reports", [], |r| r.get(0)).unwrap() };
        assert_eq!(count(&cx), 1);
        // 주제책 없는 리포트(NULL)도 중복 행이 생기지 않는다.
        let mut nt = input(vec![], vec![]);
        nt.topic_book_id = None;
        save(&cx.conn, &cx.media, &nt).unwrap();
        save(&cx.conn, &cx.media, &nt).unwrap();
        assert_eq!(count(&cx), 2);

        // 거부: 다른 학생의 관찰·사진, 존재하지 않는 id, 4개 초과, 400자 초과, 기간 오류
        assert!(save(&cx.conn, &cx.media, &input(vec![&other.id], vec![])).is_err());
        assert!(save(&cx.conn, &cx.media, &input(vec![], vec![&other_photo])).is_err());
        assert!(save(&cx.conn, &cx.media, &input(vec!["nope"], vec![])).is_err());
        let mut many = vec![];
        for i in 0..5 {
            many.push(obs(&mut cx, &[0], "2027-03-10", &format!("m{i}"), &[]).id);
        }
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(save(&cx.conn, &cx.media, &input(many, vec![])).is_err());
        let mut long = input(vec![], vec![]);
        long.message = "가".repeat(401);
        assert!(save(&cx.conn, &cx.media, &long).is_err());
        long.message = "가".repeat(400);
        assert!(save(&cx.conn, &cx.media, &long).is_ok());
        let mut bad = input(vec![], vec![]);
        bad.from = "2027-05-01".into();
        assert!(save(&cx.conn, &cx.media, &bad).is_err());
        // 자막 없는 음성은 인용할 수 없다.
        let au = attachments::add_bytes(&cx.conn, &cx.media, &crate::db::media::test_key(), &a.id, "audio", "audio/webm", b"x").unwrap();
        let mut q = input(vec![], vec![]);
        q.quote_attachment_ids = vec![au.id.clone()];
        assert!(save(&cx.conn, &cx.media, &q).is_err());
        attachments::set_transcript(&cx.conn, &cx.media, &au.id, "안녕").unwrap();
        assert_eq!(save(&cx.conn, &cx.media, &q).unwrap().quote_attachment_ids, [au.id]);

        // 삭제된 관찰은 저장된 선택에서도 빠진다.
        save(&cx.conn, &cx.media, &input(vec![&a.id, &b.id], vec![])).unwrap();
        observations::delete(&cx.conn, &b.id).unwrap();
        assert_eq!(draft_of(&cx, 0).observation_ids, std::slice::from_ref(&a.id));

        // 내보내기 표시
        let id = draft_of(&cx, 0).id.unwrap();
        mark_exported(&cx.conn, &id).unwrap();
        assert!(draft_of(&cx, 0).exported_at.is_some());
        assert!(mark_exported(&cx.conn, "nope").is_err());
    }

    #[test]
    fn habit_rates_are_computed_over_the_period() {
        let cx = seed();
        let h = habits::save(
            &cx.conn,
            &habits::HabitInput { id: None, class_id: cx.class.clone(), title: "인사하기".into(), icon: "".into(), starts_on: None, ends_on: None, active: true },
        )
        .unwrap();
        let add = |student: usize, day: &str, done: i64| {
            cx.conn
                .execute(
                    "INSERT INTO habit_checks (habit_id, student_id, checked_on, done, updated_at) VALUES (?1, ?2, ?3, ?4, 't')",
                    params![h.id, cx.ids[student], day, done],
                )
                .unwrap();
        };
        // 기간 안: 03-02 03-03 03-04(하늘 2일 함), 기간 밖: 02-20, 05-01
        add(0, "2027-03-02", 1);
        add(0, "2027-03-03", 1);
        add(0, "2027-03-04", 0);
        add(1, "2027-03-04", 1);
        add(0, "2027-02-20", 1);
        add(0, "2027-05-01", 1);
        let d = draft_of(&cx, 0);
        assert_eq!(d.habits.len(), 1);
        assert_eq!((d.habits[0].done_days, d.habits[0].school_days), (2, 3));
        // 기간을 좁히면 값이 달라진다.
        let d = draft(&cx.conn, &cx.media, &cx.ids[0], None, "2027-03-03", "2027-03-31").unwrap();
        assert_eq!((d.habits[0].done_days, d.habits[0].school_days), (1, 2));
    }

    #[test]
    fn lessons_come_from_done_sessions_in_period_and_default_period_follows_the_unit() {
        let cx = seed();
        cx.conn
            .execute_batch(&format!(
                "INSERT INTO units (id, class_id, topic_book_id, tool_id, title, created_at, updated_at) VALUES ('u', '{c}', 'g1s1-school', 'free', '학교', 't', 't');
                 INSERT INTO units (id, class_id, topic_book_id, tool_id, title, created_at, updated_at) VALUES ('u2', '{c}', 'g1s1-me', 'free', '나', 't', 't');
                 INSERT INTO unit_stages (id, unit_id, position, title) VALUES ('g', 'u', 0, '단계');
                 INSERT INTO unit_stages (id, unit_id, position, title) VALUES ('g2', 'u2', 0, '단계');",
                c = cx.class
            ))
            .unwrap();
        let mut n = 0;
        let mut add = |unit: &str, stage: &str, title: &str, status: &str, on: &str| {
            n += 1;
            cx.conn
                .execute_batch(&format!(
                    "INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, status, scheduled_on, created_at, updated_at)
                     VALUES ('s{n}', '{unit}', '{stage}', {n}, '{title}', '{status}', '{on}', 't', 't')"
                ))
                .unwrap();
        };
        add("u", "g", "학교 둘러보기", "done", "2027-03-05");
        add("u", "g", "계획만 한 차시", "planned", "2027-03-06");
        add("u", "g", "건너뜀", "skipped", "2027-03-07");
        add("u", "g", "기간 밖", "done", "2027-06-01");
        add("u2", "g2", "다른 주제책", "done", "2027-03-05");
        for i in 0..9 {
            add("u", "g", &format!("차시{i}"), "done", &format!("2027-04-{:02}", i + 1));
        }
        let d = draft_of(&cx, 0);
        assert_eq!(d.lessons.len(), MAX_LESSONS);
        assert_eq!(d.lessons[0], "학교 둘러보기");
        assert!(!d.lessons.iter().any(|t| t == "계획만 한 차시" || t == "기간 밖" || t == "다른 주제책"));
        // 주제책이 없으면 차시 목록도 없다.
        assert!(draft(&cx.conn, &cx.media, &cx.ids[0], None, FROM, TO).unwrap().lessons.is_empty());

        let today = NaiveDate::from_ymd_opt(2027, 7, 1).unwrap();
        let p = default_period(&cx.conn, &cx.class, Some("g1s1-school"), today).unwrap();
        assert_eq!((p.from.as_str(), p.to.as_str()), ("2027-03-05", "2027-06-01"));
        let p = default_period(&cx.conn, &cx.class, Some("g1s1-nothing"), today).unwrap();
        assert_eq!((p.from.as_str(), p.to.as_str()), ("2027-05-21", "2027-07-01"));
        assert_eq!(default_period(&cx.conn, &cx.class, None, today).unwrap(), p);
    }

    #[test]
    fn write_file_checks_extension_size_folder_and_overwrites() {
        let dir = tmp("out");
        let png = |extra: usize| {
            let mut v = PNG_MAGIC.to_vec();
            v.extend(std::iter::repeat_n(7u8, extra));
            v
        };
        let target = dir.join("1-김하늘-리포트.png");
        write_file(target.to_str().unwrap(), &png(10)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), png(10));
        // 덮어쓰기, .part 찌꺼기 없음
        write_file(target.to_str().unwrap(), &png(20)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), png(20));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        assert!(write_file(dir.join("a.jpg").to_str().unwrap(), &png(1)).is_err());
        assert!(write_file(dir.join("a").to_str().unwrap(), &png(1)).is_err());
        assert!(write_file(dir.join("없는폴더").join("a.png").to_str().unwrap(), &png(1)).is_err());
        assert!(write_file("a.png", &png(1)).is_err());
        assert!(write_file(target.to_str().unwrap(), b"not a png").is_err());
        assert!(write_file(target.to_str().unwrap(), &[]).is_err());
        assert!(write_file(target.to_str().unwrap(), &png(MAX_FILE_BYTES)).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), png(20), "거부된 쓰기는 기존 파일을 건드리지 않는다");
        std::fs::create_dir(dir.join("폴더.png")).unwrap();
        assert!(write_file(dir.join("폴더.png").to_str().unwrap(), &png(1)).is_err());
    }

    #[test]
    fn percent_decode_handles_korean_paths() {
        assert_eq!(percent_decode("C%3A%5C%EB%A6%AC%ED%8F%AC%ED%8A%B8%5C1-%EA%B9%80.png").unwrap(), "C:\\리포트\\1-김.png");
        assert!(percent_decode("%zz").is_err());
        assert!(percent_decode("%").is_err());
        assert!(percent_decode("%FF%FE").is_err());
    }
}
