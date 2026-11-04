//! 생기부 평어 초안(준비 단계): 근거 묶음 -> 템플릿 초안 -> 자동 검사 -> 교사 편집·확정 -> 복사·내보내기.
//! 설계: `docs/design/08-evaluation-draft.md`.
//!
//! 원칙: 근거 없는 문장은 확정할 수 없고, 다른 학생 이름은 근거 묶음에 들어가지 않으며,
//! 아이 데이터는 PC 밖으로 나가지 않는다(이 모듈에는 네트워크 요청이 없다).

pub mod bundle;
pub mod check;
pub mod data;
pub mod generate;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use self::bundle::{Bundle, MIN_EVIDENCE};
use self::check::{check_draft, DraftCheck, LEN_MAX, LEN_MIN};
use self::data::{check_subject, forbidden, SUBJECTS};
use self::generate::{generate, Sentence, Style};
use super::reports::{check_period, Scrubber};
use super::{classes, lock, new_id, now_iso, settings, students, DbExt, DbResult, MediaDir};

pub const KEY_STYLE: &str = "evalStyle";
pub const KEY_TARGET: &str = "evalTargetChars";
pub const KEY_EXTRA: &str = "evalForbiddenExtra";
pub const DEFAULT_STYLE: &str = "ham";
pub const DEFAULT_TARGET: usize = 250;
const MAX_SENTENCES: usize = 12;
const MAX_SENTENCE_CHARS: usize = 500;
const MAX_EXTRA_WORDS: usize = 200;
const MAX_EXTRA_CHARS: usize = 30;
const MAX_EXPORT_BYTES: usize = 5 * 1024 * 1024;

// ---------- 설정 ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalSettings {
    /// ham(~함) | eum(~음) | boim(~하는 모습을 보임)
    pub style: String,
    pub target_chars: usize,
    /// 교사가 더한 금지 낱말.
    pub extra_forbidden: Vec<String>,
}

fn clean_extra(words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words.iter().map(|w| w.trim()) {
        if !w.is_empty() && w.chars().count() <= MAX_EXTRA_CHARS && !out.iter().any(|o| o == w) && out.len() < MAX_EXTRA_WORDS {
            out.push(w.to_string());
        }
    }
    out
}

/// 저장된 값이 없거나 잘못되면 기본값을 쓴다.
pub fn load_settings(conn: &Connection) -> DbResult<EvalSettings> {
    let style = settings::get(conn, KEY_STYLE)?.as_str().filter(|s| Style::parse(s).is_some()).unwrap_or(DEFAULT_STYLE).to_string();
    let target_chars = settings::get(conn, KEY_TARGET)?.as_u64().map(|n| n as usize).filter(|n| (LEN_MIN..=LEN_MAX).contains(n)).unwrap_or(DEFAULT_TARGET);
    let extra: Vec<String> = settings::get(conn, KEY_EXTRA)?.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Ok(EvalSettings { style, target_chars, extra_forbidden: clean_extra(&extra) })
}

pub fn save_settings(conn: &Connection, input: &EvalSettings) -> DbResult<EvalSettings> {
    if Style::parse(&input.style).is_none() {
        return Err("문체는 ~함, ~음, ~하는 모습을 보임 중에서 골라 주세요".to_string());
    }
    if !(LEN_MIN..=LEN_MAX).contains(&input.target_chars) {
        return Err(format!("목표 글자 수는 {LEN_MIN}~{LEN_MAX}자 사이로 정해 주세요"));
    }
    let extra = clean_extra(&input.extra_forbidden);
    settings::set(conn, KEY_STYLE, &json!(input.style))?;
    settings::set(conn, KEY_TARGET, &json!(input.target_chars))?;
    settings::set(conn, KEY_EXTRA, &json!(extra))?;
    load_settings(conn)
}

// ---------- 저장된 초안 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredDraft {
    pub id: String,
    pub sentences: Vec<Sentence>,
    /// draft | confirmed
    pub status: String,
    /// template | manual (나중에 llm)
    pub generator: String,
    pub confirmed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftKey {
    pub student_id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateInput {
    #[serde(flatten)]
    pub key: DraftKey,
    /// 교사가 체크를 풀어 뺀 근거 id.
    #[serde(default)]
    pub excluded_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveInput {
    #[serde(flatten)]
    pub key: DraftKey,
    pub sentences: Vec<Sentence>,
}

/// 정규화된 (학생 id, 교과, 시작, 끝).
struct Key {
    student_id: String,
    subject: &'static str,
    from: String,
    to: String,
}

fn norm(key: &DraftKey) -> DbResult<Key> {
    let subject = check_subject(&key.subject)?;
    let (from, to, _) = check_period(&key.from, &key.to)?;
    Ok(Key { student_id: key.student_id.clone(), subject, from, to })
}

fn row_to_draft(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, String, String, Option<String>, String)> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
}

fn load_draft(conn: &Connection, k: &Key) -> DbResult<Option<StoredDraft>> {
    let row = conn
        .query_row(
            "SELECT id, sentences, status, generator, confirmed_at, updated_at FROM evaluation_drafts
             WHERE student_id = ?1 AND subject = ?2 AND period_from = ?3 AND period_to = ?4",
            params![k.student_id, k.subject, k.from, k.to],
            row_to_draft,
        )
        .optional()
        .db()?;
    row.map(|(id, sentences, status, generator, confirmed_at, updated_at)| {
        let sentences = serde_json::from_str(&sentences).map_err(|e| format!("저장된 평어를 읽을 수 없습니다: {e}"))?;
        Ok(StoredDraft { id, sentences, status, generator, confirmed_at, updated_at })
    })
    .transpose()
}

fn evidence_ids(sentences: &[Sentence]) -> Vec<String> {
    sentences.iter().flat_map(|s| s.evidence.iter().cloned()).collect()
}

fn normalize_sentences(input: &[Sentence]) -> DbResult<Vec<Sentence>> {
    if input.len() > MAX_SENTENCES {
        return Err(format!("문장은 {MAX_SENTENCES}개까지 쓸 수 있습니다"));
    }
    let mut out = Vec::new();
    for (i, s) in input.iter().enumerate() {
        let text = s.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err(format!("{}번째 문장이 비어 있습니다. 내용을 쓰거나 문장을 지워 주세요", i + 1));
        }
        if text.chars().count() > MAX_SENTENCE_CHARS {
            return Err(format!("{}번째 문장이 너무 깁니다({MAX_SENTENCE_CHARS}자까지)", i + 1));
        }
        let mut evidence: Vec<String> = Vec::new();
        for id in s.evidence.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            if id.chars().count() > 100 {
                return Err("근거 id가 올바르지 않습니다".to_string());
            }
            if !evidence.iter().any(|e| e == id) {
                evidence.push(id.to_string());
            }
        }
        out.push(Sentence { text, evidence });
    }
    Ok(out)
}

fn write_draft(conn: &Connection, student_id: &str, k: &Key, sentences: &[Sentence], generator: &str) -> DbResult<()> {
    let student = students::get(conn, student_id)?;
    let json = serde_json::to_string(sentences).expect("JSON");
    let now = now_iso();
    // 고치거나 새로 만들면 항상 초안으로 돌아간다(확정은 다시 눌러야 한다).
    conn.execute(
        "INSERT INTO evaluation_drafts (id, class_id, student_id, subject, period_from, period_to, sentences, status, generator, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'draft', ?8, ?9, ?9)
         ON CONFLICT (student_id, subject, period_from, period_to) DO UPDATE SET
           sentences = excluded.sentences, status = 'draft', confirmed_at = NULL, updated_at = excluded.updated_at,
           generator = CASE WHEN ?8 = 'manual' THEN generator ELSE excluded.generator END",
        params![new_id(), student.class_id, student.id, k.subject, k.from, k.to, json, generator, now],
    )
    .db()?;
    Ok(())
}

// ---------- 화면용 묶음 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ViewStudent {
    pub id: String,
    pub number: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DraftView {
    pub student: ViewStudent,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub bundle: Bundle,
    /// 근거가 최소 개수(3개) 이상인가.
    pub enough: bool,
    pub min_evidence: usize,
    pub draft: Option<StoredDraft>,
    pub check: Option<DraftCheck>,
}

fn other_names(conn: &Connection, student_id: &str) -> DbResult<Vec<String>> {
    let s = students::get(conn, student_id)?;
    Ok(Scrubber::new(conn, &s.class_id, student_id)?.other_names().to_vec())
}

fn run_check(conn: &Connection, student_id: &str, sentences: &[Sentence], bundle: &Bundle) -> DbResult<DraftCheck> {
    let settings = load_settings(conn)?;
    Ok(check_draft(sentences, bundle, forbidden(), &settings.extra_forbidden, &other_names(conn, student_id)?))
}

pub fn view(conn: &Connection, media: &Path, key: &DraftKey) -> DbResult<DraftView> {
    let k = norm(key)?;
    let student = students::get(conn, &k.student_id)?;
    let draft = load_draft(conn, &k)?;
    let pinned = draft.as_ref().map(|d| evidence_ids(&d.sentences)).unwrap_or_default();
    let bundle = bundle::build(conn, media, &k.student_id, k.subject, &k.from, &k.to, &pinned)?;
    let check = draft.as_ref().map(|d| run_check(conn, &k.student_id, &d.sentences, &bundle)).transpose()?;
    Ok(DraftView {
        student: ViewStudent { id: student.id, number: student.number, name: student.name },
        subject: k.subject.to_string(),
        enough: bundle.evidence.len() >= MIN_EVIDENCE,
        min_evidence: MIN_EVIDENCE,
        from: k.from,
        to: k.to,
        bundle,
        draft,
        check,
    })
}

pub fn insufficient_message(n: usize) -> String {
    format!("기록이 부족합니다({n}개). 이 교과 관찰 기록을 더 남겨 주세요")
}

/// 템플릿으로 초안을 만들어 저장한다. 근거가 3개 미만이면 만들지 않는다.
pub fn generate_draft(conn: &Connection, media: &Path, input: &GenerateInput) -> DbResult<DraftView> {
    let k = norm(&input.key)?;
    let mut b = bundle::build(conn, media, &k.student_id, k.subject, &k.from, &k.to, &[])?;
    b.evidence.retain(|e| !input.excluded_ids.iter().any(|x| x == e.id()));
    if b.evidence.len() < MIN_EVIDENCE {
        return Err(insufficient_message(b.evidence.len()));
    }
    let s = load_settings(conn)?;
    let style = Style::parse(&s.style).unwrap_or(Style::Ham);
    let sentences = generate(&b, style, s.target_chars, &k.student_id, forbidden(), &s.extra_forbidden);
    if sentences.is_empty() {
        return Err("초안을 만들 수 없습니다. 근거를 더 골라 주세요".to_string());
    }
    write_draft(conn, &k.student_id, &k, &sentences, "template")?;
    view(conn, media, &input.key)
}

/// 교사가 고친 문장을 저장한다. 확정은 풀리고 초안이 된다.
pub fn save_draft(conn: &Connection, media: &Path, input: &SaveInput) -> DbResult<DraftView> {
    let k = norm(&input.key)?;
    let sentences = normalize_sentences(&input.sentences)?;
    write_draft(conn, &k.student_id, &k, &sentences, "manual")?;
    view(conn, media, &input.key)
}

/// 저장하지 않고 문장만 검사한다(편집 중 경고 갱신).
pub fn check_sentences(conn: &Connection, media: &Path, input: &SaveInput) -> DbResult<DraftCheck> {
    let k = norm(&input.key)?;
    // 빈 문장도 검사할 수 있도록 normalize는 쓰지 않고 공백만 정리한다.
    let sentences: Vec<Sentence> = input
        .sentences
        .iter()
        .take(MAX_SENTENCES)
        .map(|s| Sentence { text: s.text.trim().to_string(), evidence: s.evidence.clone() })
        .collect();
    let bundle = bundle::build(conn, media, &k.student_id, k.subject, &k.from, &k.to, &evidence_ids(&sentences))?;
    run_check(conn, &k.student_id, &sentences, &bundle)
}

/// 확정한다. 모든 문장에 근거가 있어야 한다.
pub fn confirm(conn: &Connection, media: &Path, key: &DraftKey) -> DbResult<DraftView> {
    let k = norm(key)?;
    let draft = load_draft(conn, &k)?.ok_or("확정할 초안이 없습니다. 먼저 초안을 만들거나 문장을 저장해 주세요")?;
    if draft.sentences.is_empty() {
        return Err("문장이 없어 확정할 수 없습니다".to_string());
    }
    let bundle = bundle::build(conn, media, &k.student_id, k.subject, &k.from, &k.to, &evidence_ids(&draft.sentences))?;
    for (i, s) in draft.sentences.iter().enumerate() {
        let ok = !s.evidence.is_empty() && s.evidence.iter().all(|id| bundle.evidence.iter().any(|e| e.id() == id));
        if !ok {
            return Err(format!("{}번째 문장에 근거가 없습니다. 문장마다 근거를 하나 이상 붙여야 확정할 수 있습니다", i + 1));
        }
    }
    conn.execute("UPDATE evaluation_drafts SET status = 'confirmed', confirmed_at = ?2 WHERE id = ?1", params![draft.id, now_iso()]).db()?;
    view(conn, media, key)
}

// ---------- 학급 현황과 내보내기 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudentRow {
    pub student_id: String,
    pub number: i64,
    pub name: String,
    pub evidence_count: usize,
    /// none | draft | confirmed
    pub status: String,
}

pub fn class_status(conn: &Connection, media: &Path, class_id: &str, subject: &str, from: &str, to: &str) -> DbResult<Vec<StudentRow>> {
    let subject = check_subject(subject)?;
    let (from, to, _) = check_period(from, to)?;
    classes::get(conn, class_id)?;
    let mut stmt = conn
        .prepare("SELECT student_id, status FROM evaluation_drafts WHERE class_id = ?1 AND subject = ?2 AND period_from = ?3 AND period_to = ?4")
        .db()?;
    let statuses: HashMap<String, String> =
        stmt.query_map(params![class_id, subject, from, to], |r| Ok((r.get(0)?, r.get(1)?))).db()?.collect::<Result<_, _>>().db()?;
    let mut rows = Vec::new();
    for s in students::list(conn, class_id, false)? {
        let count = bundle::build(conn, media, &s.id, subject, &from, &to, &[])?.evidence.len();
        let status = statuses.get(&s.id).cloned().unwrap_or_else(|| "none".to_string());
        rows.push(StudentRow { student_id: s.id, number: s.number, name: s.name, evidence_count: count, status });
    }
    Ok(rows)
}

fn tsv_cell(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 학급의 확정본만 모은 텍스트(번호, 이름, 교과, 평어를 탭으로 나눔). 확정본이 없으면 오류.
pub fn export_text(conn: &Connection, class_id: &str, from: &str, to: &str) -> DbResult<(String, usize)> {
    let (from, to, _) = check_period(from, to)?;
    classes::get(conn, class_id)?;
    let mut lines = vec!["번호\t이름\t교과\t평어".to_string()];
    let mut count = 0;
    for s in students::list(conn, class_id, false)? {
        for subject in SUBJECTS {
            let k = Key { student_id: s.id.clone(), subject, from: from.clone(), to: to.clone() };
            let Some(d) = load_draft(conn, &k)?.filter(|d| d.status == "confirmed") else { continue };
            let name = data::curriculum_subject_name(subject);
            let text = d.sentences.iter().map(|x| tsv_cell(&x.text)).collect::<Vec<_>>().join(" ");
            lines.push(format!("{}\t{}\t{}\t{}", s.number, tsv_cell(&s.name), name, text));
            count += 1;
        }
    }
    if count == 0 {
        return Err("확정된 평어가 없습니다. 학생마다 「확정」을 누른 뒤 내보내 주세요".to_string());
    }
    Ok((lines.join("\r\n") + "\r\n", count))
}

/// 교사가 고른 경로에 UTF-8(BOM) 텍스트를 쓴다. `.part`에 쓴 뒤 이름을 바꿔 기존 파일을 덮어쓴다.
pub fn write_text_file(path: &str, text: &str) -> DbResult<()> {
    let target = PathBuf::from(path);
    let ext = target.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if ext != "txt" {
        return Err("평어는 .txt 파일로만 저장할 수 있습니다".to_string());
    }
    if !target.is_absolute() {
        return Err("저장 경로가 올바르지 않습니다".to_string());
    }
    if text.trim().is_empty() {
        return Err("저장할 내용이 없습니다".to_string());
    }
    let mut data = Vec::with_capacity(text.len() + 3);
    data.extend_from_slice(b"\xEF\xBB\xBF");
    data.extend_from_slice(text.as_bytes());
    if data.len() > MAX_EXPORT_BYTES {
        return Err("내용이 너무 큽니다(5MB 이하만 저장할 수 있습니다)".to_string());
    }
    let dir = target.parent().filter(|d| d.is_dir()).ok_or("저장할 폴더가 없습니다")?;
    if target.is_dir() {
        return Err("같은 이름의 폴더가 있어 저장할 수 없습니다".to_string());
    }
    let name = target.file_name().and_then(|n| n.to_str()).ok_or("저장 경로가 올바르지 않습니다")?;
    let part = dir.join(format!("{name}.part"));
    std::fs::write(&part, &data).map_err(|e| format!("파일을 저장하지 못했습니다: {e}"))?;
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("파일을 저장하지 못했습니다: {e}")
    })
}

// ---------- 명령 ----------

#[tauri::command]
pub fn eval_settings_get(db: State<Mutex<Connection>>) -> DbResult<EvalSettings> {
    load_settings(&*lock(&db)?)
}

#[tauri::command]
pub fn eval_settings_set(db: State<Mutex<Connection>>, input: EvalSettings) -> DbResult<EvalSettings> {
    save_settings(&*lock(&db)?, &input)
}

#[tauri::command]
pub fn eval_status(db: State<Mutex<Connection>>, media: State<MediaDir>, class_id: String, subject: String, from: String, to: String) -> DbResult<Vec<StudentRow>> {
    class_status(&*lock(&db)?, &media.0, &class_id, &subject, &from, &to)
}

#[tauri::command]
pub fn eval_view(db: State<Mutex<Connection>>, media: State<MediaDir>, key: DraftKey) -> DbResult<DraftView> {
    view(&*lock(&db)?, &media.0, &key)
}

#[tauri::command]
pub fn eval_generate(db: State<Mutex<Connection>>, media: State<MediaDir>, input: GenerateInput) -> DbResult<DraftView> {
    generate_draft(&*lock(&db)?, &media.0, &input)
}

#[tauri::command]
pub fn eval_save(db: State<Mutex<Connection>>, media: State<MediaDir>, input: SaveInput) -> DbResult<DraftView> {
    save_draft(&*lock(&db)?, &media.0, &input)
}

#[tauri::command]
pub fn eval_check(db: State<Mutex<Connection>>, media: State<MediaDir>, input: SaveInput) -> DbResult<DraftCheck> {
    check_sentences(&*lock(&db)?, &media.0, &input)
}

#[tauri::command]
pub fn eval_confirm(db: State<Mutex<Connection>>, media: State<MediaDir>, key: DraftKey) -> DbResult<DraftView> {
    confirm(&*lock(&db)?, &media.0, &key)
}

/// 학급 확정본을 고른 경로의 .txt로 저장하고 내보낸 개수를 돌려준다.
#[tauri::command]
pub fn eval_export(db: State<Mutex<Connection>>, class_id: String, from: String, to: String, path: String) -> DbResult<usize> {
    let (text, n) = export_text(&*lock(&db)?, &class_id, &from, &to)?;
    write_text_file(&path, &text)?;
    Ok(n)
}

#[cfg(test)]
mod tests;
