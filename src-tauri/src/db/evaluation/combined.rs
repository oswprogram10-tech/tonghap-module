//! 세 교과(바·슬·즐) 통합 평어. 설계: `docs/design/18-integrated-evaluation.md`.
//!
//! 1~2학년은 NEIS에 세 교과를 통합해 입력한다. 생성·근거·확정은 교과별로 하고,
//! 확정된 세 교과 평어를 이어 붙여 학생별 통합본을 만든다. 선생님이 고친 통합본은 자동으로 덮지 않는다.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::check::{Warning, FORBIDDEN, OTHER_NAME};
use super::data::{curriculum_subject_name, forbidden, SUBJECTS};
use super::{load_draft, load_settings, other_names, tsv_cell, Key, ViewStudent};
use crate::db::reports::check_period;
use crate::db::{classes, new_id, now_iso, students, DbExt, DbResult};

pub const NOUN_ENDING: &str = "nounEnding";
pub const SYMBOL: &str = "symbol";
pub const MISSING_SUBJECT: &str = "missingSubject";

/// 이 글자 수를 넘으면 학교 방침 확인 안내를 보여 준다(차단하지 않는다).
pub const NOTICE_CHARS: usize = 500;
const MAX_TEXT_CHARS: usize = 5000;

// ---------- 글자 수와 NEIS 바이트 ----------

/// 글자 수(줄바꿈 한 개를 한 글자로 센다. `\r`은 세지 않는다).
pub fn count_chars(text: &str) -> usize {
    text.chars().filter(|c| *c != '\r').count()
}

/// NEIS 바이트: 한글 3, 영문·숫자·ASCII 1, 줄바꿈 1(`\r`은 세지 않는다). 그 밖의 비ASCII 글자는 3.
pub fn neis_bytes(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '\r' => 0,
            '\n' => 1,
            c if c.is_ascii() => 1,
            _ => 3,
        })
        .sum()
}

// ---------- 새 검사 ----------

/// 한글 음절의 받침이 ㅁ(번호 16) 또는 ㄻ(번호 10)인가(함·음·됨·봄 ... 만듦·앎·삶).
fn ends_with_mieum(c: char) -> bool {
    let n = c as u32;
    (0xAC00..=0xD7A3).contains(&n) && matches!((n - 0xAC00) % 28, 10 | 16)
}

fn is_closer(c: char) -> bool {
    matches!(c, '"' | '\'' | ')' | ']' | '”' | '’' | '」' | '』' | '）' | '〕' | '>' | '》')
}

/// 마침표(`.`, `。`, `!`, `?`)와 줄바꿈으로 나눈 문장. `3.5`처럼 숫자 사이의 점은 나누지 않는다.
pub fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let decimal = c == '.' && i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        if !decimal && matches!(c, '.' | '。' | '!' | '?' | '\n' | '\r') {
            let t = cur.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

/// 문장이 명사형으로 끝나는가. 글자가 하나도 없는 조각(기호뿐)은 검사하지 않는다.
pub fn ends_with_noun_form(sentence: &str) -> bool {
    let core = sentence.trim().trim_end_matches(|c: char| c.is_whitespace() || is_closer(c));
    match core.chars().last() {
        Some(c) => ends_with_mieum(c) || !c.is_alphanumeric(),
        None => true,
    }
}

fn tail(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(10);
    let t: String = chars[start..].iter().collect();
    if start > 0 {
        format!("…{t}")
    } else {
        t
    }
}

fn is_bullet(c: char) -> bool {
    matches!(c, '•' | '·' | '※' | '*' | '-' | '–' | '—' | '▶' | '▷' | '►' | '★' | '☆' | '●' | '○' | '■' | '□' | '◆' | '◇' | '▪' | '▫' | '✔' | '✓' | '→')
}

fn is_emoji(c: char) -> bool {
    let n = c as u32;
    (0x1F000..=0x1FAFF).contains(&n) || (0x2600..=0x27BF).contains(&n) || (0x2B00..=0x2BFF).contains(&n) || n == 0xFE0F || n == 0x200D
}

fn is_circled_number(c: char) -> bool {
    let n = c as u32;
    (0x2460..=0x2473).contains(&n) || (0x3251..=0x325F).contains(&n) || (0x24EB..=0x24FF).contains(&n) || (0x2776..=0x2793).contains(&n)
}

/// 줄머리에 `1.` `2)` `(3)`처럼 번호가 있는가.
fn starts_with_number(line: &str) -> bool {
    let t = line.trim_start().trim_start_matches(['(', '（']);
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && matches!(t.chars().nth(digits), Some('.' | ')' | '）'))
}

/// 번호·줄머리표·특수문자를 찾아 눈에 띄는 것을 돌려준다(중복 제거, 최대 6개).
pub fn find_symbols(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut add = |s: String| {
        if !found.contains(&s) && found.len() < 6 {
            found.push(s);
        }
    };
    for line in text.lines() {
        let first = line.trim_start().chars().next();
        if let Some(c) = first {
            if is_bullet(c) {
                add(c.to_string());
            }
        }
        if starts_with_number(line) {
            let t = line.trim_start();
            add(t.chars().take_while(|c| !c.is_whitespace()).take(4).collect());
        }
    }
    for c in text.chars() {
        if is_circled_number(c) || is_emoji(c) || matches!(c, '•' | '※' | '*' | '▶' | '▷' | '►' | '★' | '☆' | '●' | '○' | '■' | '□' | '◆' | '◇') {
            add(c.to_string());
        }
    }
    found
}

// ---------- 통합본 ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedKey {
    pub student_id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombineInput {
    #[serde(flatten)]
    pub key: CombinedKey,
    /// 이미 있는 통합본을 새로 합친 글로 바꿀 때만 true(화면에서 확인을 받은 뒤).
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedTextInput {
    #[serde(flatten)]
    pub key: CombinedKey,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubjectState {
    pub code: String,
    pub name: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredCombined {
    pub id: String,
    pub text: String,
    /// draft | confirmed
    pub status: String,
    pub confirmed_at: Option<String>,
    pub updated_at: String,
    /// 만든 뒤 교과 평어가 바뀌었다(다시 합치기를 안내한다).
    pub stale: bool,
    /// 선생님이 「근거 확인」을 체크했다(확정의 조건). 합칠 때마다 풀리고, 글을 고쳐도 유지된다.
    pub reviewed: bool,
    /// 언어 모델이 쓴 교과 평어가 섞였는데, 합친 글을 한 글자도 고치지 않았다.
    pub unchanged_from_model: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CombinedCheck {
    pub warnings: Vec<Warning>,
    pub chars: usize,
    pub bytes: usize,
    /// 500자를 넘을 때의 안내(차단하지 않는다).
    pub length_notice: Option<String>,
    /// 금지 표현·다른 학생 이름이 없고 글이 있다.
    pub can_confirm: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CombinedView {
    pub student: ViewStudent,
    pub from: String,
    pub to: String,
    pub subjects: Vec<SubjectState>,
    /// 세 교과가 모두 확정이라 합칠 수 있다.
    pub can_combine: bool,
    pub combined: Option<StoredCombined>,
    pub check: Option<CombinedCheck>,
}

pub struct Sources {
    /// 바·슬·즐 순서. 확정되지 않았으면 None.
    texts: Vec<Option<String>>,
    /// 세 교과 중 언어 모델이 쓴 초안이 있다.
    any_llm: bool,
}

impl Sources {
    fn missing(&self) -> Vec<&'static str> {
        SUBJECTS.iter().zip(&self.texts).filter(|(_, t)| t.is_none()).map(|(s, _)| curriculum_subject_name(s)).collect()
    }
    fn all_confirmed(&self) -> bool {
        self.texts.iter().all(Option::is_some)
    }
    fn joined(&self) -> String {
        self.texts.iter().flatten().cloned().collect::<Vec<_>>().join(" ")
    }
    /// 바뀌면 달라지는 값. FNV-1a 64비트(저장되므로 버전에 따라 바뀌지 않는 알고리즘을 쓴다).
    fn hash(&self) -> String {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self.texts.iter().flatten().flat_map(|t| t.bytes().chain(std::iter::once(0x1f))) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{h:016x}")
    }
}

fn load_sources(conn: &Connection, student_id: &str, from: &str, to: &str) -> DbResult<Sources> {
    let mut texts = Vec::new();
    let mut any_llm = false;
    for subject in SUBJECTS {
        let k = Key { student_id: student_id.to_string(), subject, from: from.to_string(), to: to.to_string() };
        let draft = load_draft(conn, &k)?.filter(|d| d.status == "confirmed" && !d.sentences.is_empty());
        any_llm |= draft.as_ref().is_some_and(|d| d.generator.starts_with("llm:"));
        texts.push(draft.map(|d| d.sentences.iter().map(|s| tsv_cell(&s.text)).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")));
    }
    Ok(Sources { texts, any_llm })
}

fn norm(key: &CombinedKey) -> DbResult<(String, String)> {
    let (from, to, _) = check_period(&key.from, &key.to)?;
    Ok((from, to))
}

struct Row {
    id: String,
    text: String,
    source_hash: String,
    status: String,
    confirmed_at: Option<String>,
    updated_at: String,
    reviewed: bool,
    generated_text: Option<String>,
}

fn load_row(conn: &Connection, student_id: &str, from: &str, to: &str) -> DbResult<Option<Row>> {
    conn.query_row(
        "SELECT id, text, source_hash, status, confirmed_at, updated_at, reviewed, generated_text FROM evaluation_combined WHERE student_id = ?1 AND period_from = ?2 AND period_to = ?3",
        params![student_id, from, to],
        |r| Ok(Row {
            id: r.get(0)?,
            text: r.get(1)?,
            source_hash: r.get(2)?,
            status: r.get(3)?,
            confirmed_at: r.get(4)?,
            updated_at: r.get(5)?,
            reviewed: r.get::<_, i64>(6)? != 0,
            generated_text: r.get(7)?,
        }),
    )
    .optional()
    .db()
}

fn is_stale(row: &Row, sources: &Sources) -> bool {
    !sources.all_confirmed() || sources.hash() != row.source_hash
}

/// 글만 검사한다. `sources`가 있으면 그 교과에서 온 문장이 하나도 남지 않았는지도 본다.
pub fn check_text(conn: &Connection, student_id: &str, text: &str, sources: Option<&Sources>) -> DbResult<CombinedCheck> {
    let settings = load_settings(conn)?;
    let mut warnings: Vec<Warning> = Vec::new();
    let w = |code: &str, message: String| Warning { code: code.to_string(), message };

    for h in forbidden().find(text, &settings.extra_forbidden) {
        warnings.push(w(FORBIDDEN, format!("금지 표현({}): '{}'", h.label, h.matched)));
    }
    let mut seen: Vec<&String> = Vec::new();
    let names = other_names(conn, student_id)?;
    for n in names.iter().filter(|n| n.chars().count() >= 2 && text.contains(n.as_str())) {
        if !seen.contains(&n) {
            seen.push(n);
            warnings.push(w(OTHER_NAME, format!("다른 학생 이름이 들어 있습니다: '{n}'")));
        }
    }

    let bad: Vec<String> = split_sentences(text).into_iter().filter(|s| !ends_with_noun_form(s)).collect();
    for s in bad.iter().take(5) {
        warnings.push(w(NOUN_ENDING, format!("명사형 어미(~함, ~음)로 끝내 주세요: '{}'", tail(s))));
    }
    if bad.len() > 5 {
        warnings.push(w(NOUN_ENDING, format!("명사형 어미로 끝나지 않는 문장이 {}개 더 있습니다", bad.len() - 5)));
    }

    let symbols = find_symbols(text);
    if !symbols.is_empty() {
        warnings.push(w(SYMBOL, format!("특수문자·번호 기호는 쓰지 않는 것이 좋습니다: {}", symbols.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(" "))));
    }

    if let Some(src) = sources {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for (subject, t) in SUBJECTS.iter().zip(&src.texts) {
            let Some(t) = t else { continue };
            let sentences = split_sentences(t);
            if !sentences.is_empty() && !sentences.iter().any(|s| flat.contains(s.as_str())) {
                warnings.push(w(MISSING_SUBJECT, format!("{} 내용이 빠진 것 같습니다. 세 교과가 모두 들어갔는지 확인해 주세요", curriculum_subject_name(subject))));
            }
        }
    }

    let chars = count_chars(text);
    let length_notice = (chars > NOTICE_CHARS).then(|| {
        format!("{chars}자입니다. 학교 방침에 맞는지 확인하세요(2024 기재요령 기준 교과별 500자, 2026 초등 기재요령은 시스템 범위 안에서 별도 제한 없음)")
    });
    let can_confirm = !text.trim().is_empty() && !warnings.iter().any(|x| x.code == FORBIDDEN || x.code == OTHER_NAME);
    Ok(CombinedCheck { warnings, chars, bytes: neis_bytes(text), length_notice, can_confirm })
}

pub fn view(conn: &Connection, key: &CombinedKey) -> DbResult<CombinedView> {
    let (from, to) = norm(key)?;
    let student = students::get(conn, &key.student_id)?;
    let sources = load_sources(conn, &student.id, &from, &to)?;
    let row = load_row(conn, &student.id, &from, &to)?;
    let check = row.as_ref().map(|r| check_text(conn, &student.id, &r.text, Some(&sources))).transpose()?;
    Ok(CombinedView {
        student: ViewStudent { id: student.id.clone(), number: student.number, name: student.name.clone() },
        from,
        to,
        subjects: SUBJECTS
            .iter()
            .zip(&sources.texts)
            .map(|(s, t)| SubjectState { code: s.to_string(), name: curriculum_subject_name(s).to_string(), confirmed: t.is_some() })
            .collect(),
        can_combine: sources.all_confirmed(),
        combined: row.map(|r| {
            let stale = is_stale(&r, &sources);
            let unchanged_from_model = sources.any_llm && r.generated_text.as_deref().is_some_and(|g| g == r.text);
            StoredCombined { id: r.id, text: r.text, status: r.status, confirmed_at: r.confirmed_at, updated_at: r.updated_at, stale, reviewed: r.reviewed, unchanged_from_model }
        }),
        check,
    })
}

/// 세 교과 확정본을 바 → 슬 → 즐 순서로 이어 통합본을 만든다. 교과 이름·기호는 넣지 않는다.
pub fn combine(conn: &Connection, input: &CombineInput) -> DbResult<CombinedView> {
    let (from, to) = norm(&input.key)?;
    let student = students::get(conn, &input.key.student_id)?;
    let sources = load_sources(conn, &student.id, &from, &to)?;
    let missing = sources.missing();
    if !missing.is_empty() {
        return Err(format!("{} 평어를 먼저 확정해 주세요", missing.join(", ")));
    }
    let text = sources.joined();
    let hash = sources.hash();
    let now = now_iso();
    match load_row(conn, &student.id, &from, &to)? {
        Some(_) if !input.overwrite => {
            return Err("이미 통합본이 있습니다. 새로 합치면 지금까지 고친 내용이 사라집니다".to_string());
        }
        Some(r) => {
            conn.execute(
                "UPDATE evaluation_combined SET text = ?2, source_hash = ?3, status = 'draft', confirmed_at = NULL, updated_at = ?4, reviewed = 0, generated_text = ?2 WHERE id = ?1",
                params![r.id, text, hash, now],
            )
            .db()?;
        }
        None => {
            conn.execute(
                "INSERT INTO evaluation_combined (id, class_id, student_id, period_from, period_to, text, source_hash, status, created_at, updated_at, reviewed, generated_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'draft', ?8, ?8, 0, ?6)",
                params![new_id(), student.class_id, student.id, from, to, text, hash, now],
            )
            .db()?;
        }
    }
    view(conn, &input.key)
}

fn key_of(conn: &Connection, id: &str) -> DbResult<CombinedKey> {
    conn.query_row("SELECT student_id, period_from, period_to FROM evaluation_combined WHERE id = ?1", [id], |r| {
        Ok(CombinedKey { student_id: r.get(0)?, from: r.get(1)?, to: r.get(2)? })
    })
    .optional()
    .db()?
    .ok_or_else(|| "통합본을 찾을 수 없습니다. 먼저 세 교과를 합쳐 주세요".to_string())
}

/// 선생님이 다듬은 글을 저장한다. 상태는 초안으로 돌아가고, 합칠 때의 해시는 그대로 둔다.
/// `reviewed`가 있으면 「근거 확인」 체크를 그 값으로 바꾸고, 없으면 지금 값을 유지한다(고쳐 써도 체크는 유지).
pub fn save(conn: &Connection, id: &str, text: &str, reviewed: Option<bool>) -> DbResult<CombinedView> {
    let key = key_of(conn, id)?;
    let text = text.replace("\r\n", "\n").trim().to_string();
    if text.is_empty() {
        return Err("통합본이 비어 있습니다. 내용을 쓰거나 「세 교과 합치기」를 다시 눌러 주세요".to_string());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("통합본이 너무 깁니다({MAX_TEXT_CHARS}자까지)"));
    }
    conn.execute(
        "UPDATE evaluation_combined SET text = ?2, status = 'draft', confirmed_at = NULL, updated_at = ?3, reviewed = COALESCE(?4, reviewed) WHERE id = ?1",
        params![id, text, now_iso(), reviewed.map(i64::from)],
    )
    .db()?;
    view(conn, &key)
}

/// 검사만 한다(저장하지 않음). 편집 중 경고를 갱신할 때 쓴다.
pub fn check(conn: &Connection, input: &CombinedTextInput) -> DbResult<CombinedCheck> {
    let (from, to) = norm(&input.key)?;
    students::get(conn, &input.key.student_id)?;
    let sources = load_sources(conn, &input.key.student_id, &from, &to)?;
    check_text(conn, &input.key.student_id, &input.text, Some(&sources))
}

/// 확정한다. 금지 표현·다른 학생 이름이 남아 있으면 확정할 수 없다.
pub fn confirm(conn: &Connection, id: &str) -> DbResult<CombinedView> {
    let key = key_of(conn, id)?;
    let (from, to) = norm(&key)?;
    let row = load_row(conn, &key.student_id, &from, &to)?.ok_or("통합본을 찾을 수 없습니다")?;
    let sources = load_sources(conn, &key.student_id, &from, &to)?;
    let c = check_text(conn, &key.student_id, &row.text, Some(&sources))?;
    if !c.can_confirm {
        return Err("금지 표현이나 다른 학생 이름이 들어 있어 확정할 수 없습니다. 경고를 확인하고 고쳐 주세요".to_string());
    }
    if !row.reviewed {
        return Err("통합본의 「근거 확인」을 체크해 주세요. AI나 템플릿이 만든 문장은 근거와 실제 모습을 확인한 뒤 확정할 수 있습니다".to_string());
    }
    conn.execute("UPDATE evaluation_combined SET status = 'confirmed', confirmed_at = ?2 WHERE id = ?1", params![id, now_iso()]).db()?;
    view(conn, &key)
}

/// 확정을 풀고 초안으로 돌린다.
pub fn unconfirm(conn: &Connection, id: &str) -> DbResult<CombinedView> {
    let key = key_of(conn, id)?;
    conn.execute("UPDATE evaluation_combined SET status = 'draft', confirmed_at = NULL, updated_at = ?2 WHERE id = ?1", params![id, now_iso()]).db()?;
    view(conn, &key)
}

// ---------- 학급 현황과 내보내기 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CombinedRow {
    pub student_id: String,
    /// none | draft | confirmed
    pub status: String,
    pub stale: bool,
}

pub fn class_combined_status(conn: &Connection, class_id: &str, from: &str, to: &str) -> DbResult<HashMap<String, CombinedRow>> {
    let (from, to, _) = check_period(from, to)?;
    classes::get(conn, class_id)?;
    let mut out = HashMap::new();
    for s in students::list(conn, class_id, false)? {
        let row = load_row(conn, &s.id, &from, &to)?;
        let entry = match row {
            None => CombinedRow { student_id: s.id.clone(), status: "none".to_string(), stale: false },
            Some(r) => {
                let stale = is_stale(&r, &load_sources(conn, &s.id, &from, &to)?);
                CombinedRow { student_id: s.id.clone(), status: r.status, stale }
            }
        };
        out.insert(s.id, entry);
    }
    Ok(out)
}

/// 학급의 확정된 통합본만 모은 텍스트(번호, 이름, 성취수준 및 특기사항을 탭으로 나눔). 없으면 오류.
pub fn export_text(conn: &Connection, class_id: &str, from: &str, to: &str) -> DbResult<(String, usize)> {
    let (from, to, _) = check_period(from, to)?;
    classes::get(conn, class_id)?;
    let mut lines = vec!["번호\t이름\t성취수준 및 특기사항".to_string()];
    let mut count = 0;
    for s in students::list(conn, class_id, false)? {
        let Some(r) = load_row(conn, &s.id, &from, &to)?.filter(|r| r.status == "confirmed") else { continue };
        lines.push(format!("{}\t{}\t{}", s.number, tsv_cell(&s.name), tsv_cell(&r.text)));
        count += 1;
    }
    if count == 0 {
        return Err("확정된 통합본이 없습니다. 학생마다 통합본의 「확정」을 누른 뒤 내보내 주세요".to_string());
    }
    Ok((lines.join("\r\n") + "\r\n", count))
}
