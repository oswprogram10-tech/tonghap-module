//! 활동 카드: 기본 카드(`data/activities/activities.json`), 교사 카드, 꼬리 잇기 추천.
//!
//! 카드 검증 규칙은 `data/activities/schema/activities.schema.json`과 같다.
//! 추천은 언어 모델 없이 규칙(학생 질문 낱말, 아직 안 다룬 성취기준, 필수 편성 부족)으로 점수를 매긴다.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::evaluation::check::{tokens, variants};
use super::evaluation::data::curriculum;
use super::units::{self, Session, UnitDetail};
use super::{classes, lock, new_id, now_iso, DbExt, DbResult};

const BUILTIN_JSON: &str = include_str!("../../../data/activities/activities.json");
const KINDS: [&str; 3] = ["general", "safety", "makingbook"];
const SUBJECTS: [&str; 3] = ["바", "슬", "즐"];
/// 주제책마다 채워야 하는 차시 수(교육과정 데이터 `requiredModulesPerTopicBook`).
const SAFETY_NEED: i64 = 4;
const MAKINGBOOK_NEED: i64 = 3;
const SESSION_MINUTES: i64 = 40;
const MAX_RECOMMENDATIONS: usize = 3;
const MAX_WORD_MATCHES: usize = 3;

const POINTS_WORD: i64 = 3;
const POINTS_STANDARD: i64 = 2;
const POINTS_REQUIRED: i64 = 3;
const POINTS_VARIETY: i64 = 1;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    #[serde(default)]
    pub id: String,
    pub topic_book_id: String,
    pub title: String,
    pub subject: String,
    pub standards: Vec<String>,
    pub kind: String,
    pub minutes: i64,
    pub steps: Vec<String>,
    #[serde(default)]
    pub materials: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BuiltinFile {
    activities: Vec<Activity>,
}

/// 앱에 포함된 기본 카드.
pub fn builtin() -> &'static [Activity] {
    static B: OnceLock<Vec<Activity>> = OnceLock::new();
    B.get_or_init(|| serde_json::from_str::<BuiltinFile>(BUILTIN_JSON).expect("data/activities/activities.json").activities)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendInput {
    pub unit_id: String,
    #[serde(default)]
    pub after_session_id: Option<String>,
    #[serde(default)]
    pub questions: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Recommendation {
    pub activity: Activity,
    pub score: i64,
    pub reasons: Vec<String>,
}

// ───────── 검증 ─────────

fn char_len(s: &str) -> usize {
    s.trim().chars().count()
}

/// 주제책에 대응하는 교과별 성취기준 코드(`[2바01-01]`, `[2슬01-01]`, `[2즐01-01]`).
fn topic_standards(topic_book_id: &str) -> Option<Vec<String>> {
    let t = curriculum().topic_books.iter().find(|t| t.id == topic_book_id)?;
    Some(SUBJECTS.iter().map(|s| format!("[2{s}{:02}-{:02}]", t.area, t.standard_index)).collect())
}

/// 스키마와 같은 규칙으로 검사하고, 공백을 다듬은 카드를 돌려준다.
pub fn validate(card: &Activity) -> DbResult<Activity> {
    let mut a = card.clone();
    a.title = a.title.trim().to_string();
    a.topic_book_id = a.topic_book_id.trim().to_string();
    let n = char_len(&a.title);
    if !(4..=30).contains(&n) {
        return Err("카드 제목은 4~30자로 적어 주세요".to_string());
    }
    let own = topic_standards(&a.topic_book_id).ok_or("주제책을 찾을 수 없습니다. 주제책을 골라 주세요")?;
    if !SUBJECTS.contains(&a.subject.as_str()) {
        return Err("교과는 바·슬·즐 중에서 골라 주세요".to_string());
    }
    a.standards = a.standards.iter().map(|s| s.trim().to_string()).collect();
    if a.standards.is_empty() || a.standards.len() > 3 {
        return Err("성취기준은 1~3개를 골라 주세요".to_string());
    }
    for (i, code) in a.standards.iter().enumerate() {
        if !own.contains(code) {
            return Err(format!("{code}은(는) 이 주제책의 성취기준이 아닙니다 ({})", own.join(", ")));
        }
        if a.standards[..i].contains(code) {
            return Err("같은 성취기준을 두 번 고를 수 없습니다".to_string());
        }
    }
    if !a.standards[0].starts_with(&format!("[2{}", a.subject)) {
        return Err(format!("첫째 성취기준은 {} 교과의 것이어야 합니다", a.subject));
    }
    if !KINDS.contains(&a.kind.as_str()) {
        return Err("카드 종류는 일반·안전·메이킹북 중에서 골라 주세요".to_string());
    }
    if a.minutes != 40 && a.minutes != 80 {
        return Err("카드 시간은 40분 또는 80분이어야 합니다".to_string());
    }
    a.steps = a.steps.iter().map(|s| s.trim().to_string()).collect();
    if !(3..=5).contains(&a.steps.len()) || a.steps.iter().any(|s| !(6..=60).contains(&s.chars().count())) {
        return Err("진행 순서는 3~5줄, 한 줄은 6~60자로 적어 주세요".to_string());
    }
    a.materials = a.materials.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if a.materials.iter().any(|m| m.chars().count() > 20) {
        return Err("준비물 한 개는 20자 이하로 적어 주세요".to_string());
    }
    a.keywords = a.keywords.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if !(3..=8).contains(&a.keywords.len()) || a.keywords.iter().any(|k| k.chars().count() > 10) {
        return Err("낱말은 3~8개, 하나는 10자 이하로 적어 주세요".to_string());
    }
    Ok(a)
}

// ───────── 교사 카드 ─────────

fn parse(data: &str) -> DbResult<Activity> {
    serde_json::from_str(data).map_err(|e| format!("저장된 활동 카드를 읽을 수 없습니다: {e}"))
}

pub fn list_custom(conn: &Connection) -> DbResult<Vec<Activity>> {
    let mut stmt = conn.prepare("SELECT id, data FROM custom_activities ORDER BY created_at, id").db()?;
    let rows: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).db()?.collect::<Result<_, _>>().db()?;
    rows.iter()
        .map(|(id, d)| {
            let mut a = parse(d)?;
            a.id = id.clone();
            Ok(a)
        })
        .collect()
}

/// id가 없으면 새로 만들고(`custom-<uuid>`), 있으면 고친다. 교사 카드만 고칠 수 있다.
pub fn save_custom(conn: &Connection, card: &Activity) -> DbResult<Activity> {
    let mut a = validate(card)?;
    a.id = a.id.trim().to_string();
    let now = now_iso();
    if a.id.is_empty() {
        a.id = format!("custom-{}", new_id());
        let data = serde_json::to_string(&a).map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO custom_activities (id, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)", params![a.id, data, now]).db()?;
    } else {
        if !a.id.starts_with("custom-") {
            return Err("기본 카드는 고칠 수 없습니다. 복사해서 새 카드로 저장해 주세요".to_string());
        }
        let data = serde_json::to_string(&a).map_err(|e| e.to_string())?;
        let n = conn.execute("UPDATE custom_activities SET data = ?2, updated_at = ?3 WHERE id = ?1", params![a.id, data, now]).db()?;
        if n == 0 {
            return Err("활동 카드를 찾을 수 없습니다".to_string());
        }
    }
    Ok(a)
}

pub fn delete_custom(conn: &Connection, id: &str) -> DbResult<()> {
    let n = conn.execute("DELETE FROM custom_activities WHERE id = ?1", [id]).db()?;
    if n == 0 {
        return Err("활동 카드를 찾을 수 없습니다".to_string());
    }
    Ok(())
}

/// 기본 카드와 교사 카드에서 id로 찾는다.
pub fn find(conn: &Connection, id: &str) -> DbResult<Activity> {
    if let Some(a) = builtin().iter().find(|a| a.id == id) {
        return Ok(a.clone());
    }
    list_custom(conn)?.into_iter().find(|a| a.id == id).ok_or_else(|| "활동 카드를 찾을 수 없습니다".to_string())
}

/// 차시 활동 내용에 넣는 글: 진행 순서를 번호 목록으로, 준비물이 있으면 한 줄 더.
pub fn plan_text(card: &Activity) -> String {
    let mut text = card.steps.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect::<Vec<_>>().join("\n");
    if !card.materials.is_empty() {
        text.push_str(&format!("\n\n준비물: {}", card.materials.join(", ")));
    }
    text
}

// ───────── 추천 ─────────

fn all_sessions(unit: &UnitDetail) -> Vec<&Session> {
    unit.stages.iter().flat_map(|g| g.sessions.iter()).collect()
}

/// 질문에서 카드와 맞는 낱말(조사를 뗀 꼴 또는 키워드)을 질문에 나온 순서대로, 겹치지 않게 찾는다.
fn matched_words(questions: &str, card: &Activity) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for token in tokens(questions) {
        let hit = variants(&token)
            .into_iter()
            .find(|v| v.chars().count() >= 2 && (card.title.contains(v.as_str()) || card.keywords.iter().any(|k| k.contains(v.as_str()))));
        // 어미가 길게 붙은 말('곤충이래요')은 키워드로 시작하면 일치로 본다.
        let hit = hit.or_else(|| {
            card.keywords.iter().filter(|k| k.chars().count() >= 2 && token.starts_with(k.as_str())).max_by_key(|k| k.chars().count()).cloned()
        });
        if let Some(word) = hit {
            if !found.contains(&word) {
                found.push(word);
            }
        }
    }
    found.truncate(MAX_WORD_MATCHES);
    found
}

/// 받침이 있으면 "과", 없으면 "와".
fn wa_gwa(word: &str) -> &'static str {
    match word.chars().last() {
        Some(c) if ('\u{AC00}'..='\u{D7A3}').contains(&c) && (c as u32 - 0xAC00) % 28 != 0 => "과",
        _ => "와",
    }
}

fn lacking_sessions(minutes: i64, need: i64) -> i64 {
    let rest = need * SESSION_MINUTES - minutes;
    (rest + SESSION_MINUTES - 1).div_euclid(SESSION_MINUTES)
}

/// 카드 목록을 받아 계산하는 순수 함수(테스트에서 카드 목록을 바꿔 쓴다).
pub fn recommend_with(conn: &Connection, builtin_cards: &[Activity], input: &RecommendInput) -> DbResult<Vec<Recommendation>> {
    let unit = units::get(conn, &input.unit_id)?;
    let sessions = all_sessions(&unit);
    let after = match input.after_session_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => Some(sessions.iter().copied().find(|s| s.id == id).ok_or("이 단원의 차시가 아닙니다")?),
        None => None,
    };
    let questions = match input.questions.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => q.to_string(),
        None => after.map(|s| s.student_questions.clone()).unwrap_or_default(),
    };

    // 후보: 단원 주제책의 카드(없으면 학급 학년의 모든 주제책), 이미 쓴 카드는 뺀다.
    let mut cards: Vec<Activity> = builtin_cards.to_vec();
    cards.extend(list_custom(conn)?);
    match unit.topic_book_id.as_deref() {
        Some(topic) => cards.retain(|a| a.topic_book_id == topic),
        None => {
            let prefix = format!("g{}s", classes::get(conn, &unit.class_id)?.grade);
            cards.retain(|a| a.topic_book_id.starts_with(&prefix));
        }
    }
    let used: HashSet<&str> = sessions.iter().filter_map(|s| s.activity_id.as_deref()).collect();
    cards.retain(|a| !used.contains(a.id.as_str()));

    let covered: HashSet<&str> = unit.totals.standards.iter().map(|s| s.code.as_str()).collect();
    // 앞 차시의 교과: 카드로 만든 차시면 그 카드의 교과, 아니면 성취기준의 교과.
    let mut prev_subjects: HashSet<String> = HashSet::new();
    if let Some(s) = after {
        if let Some(card) = s.activity_id.as_deref().and_then(|id| find(conn, id).ok()) {
            prev_subjects.insert(card.subject);
        } else {
            prev_subjects.extend(s.standards.iter().filter_map(|st| st.code.chars().nth(2)).map(|c| c.to_string()));
        }
    }
    let safety_lack = lacking_sessions(unit.totals.by_kind.safety, SAFETY_NEED);
    let making_lack = lacking_sessions(unit.totals.by_kind.makingbook, MAKINGBOOK_NEED);

    let mut out: Vec<Recommendation> = cards
        .into_iter()
        .map(|activity| {
            let mut score = 0;
            let mut reasons = Vec::new();
            for word in matched_words(&questions, &activity) {
                score += POINTS_WORD;
                reasons.push(format!("학생 질문의 '{word}'{} 이어져요", wa_gwa(&word)));
            }
            for code in activity.standards.iter().filter(|c| !covered.contains(c.as_str())) {
                score += POINTS_STANDARD;
                reasons.push(format!("{code}를 아직 다루지 않았어요"));
            }
            if activity.kind == "safety" && safety_lack > 0 {
                score += POINTS_REQUIRED;
                reasons.push(format!("안전 더하기가 {safety_lack}차시 더 필요해요"));
            }
            if activity.kind == "makingbook" && making_lack > 0 {
                score += POINTS_REQUIRED;
                reasons.push(format!("메이킹북이 {making_lack}차시 더 필요해요"));
            }
            if !prev_subjects.is_empty() && !prev_subjects.contains(&activity.subject) {
                score += POINTS_VARIETY;
            }
            Recommendation { activity, score, reasons }
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.activity.id.cmp(&b.activity.id)));
    out.truncate(MAX_RECOMMENDATIONS);
    Ok(out)
}

pub fn recommend(conn: &Connection, input: &RecommendInput) -> DbResult<Vec<Recommendation>> {
    recommend_with(conn, builtin(), input)
}

// ───────── Tauri 명령 ─────────

#[tauri::command]
pub fn activity_custom_list(db: State<Mutex<Connection>>) -> DbResult<Vec<Activity>> {
    list_custom(&*lock(&db)?)
}

#[tauri::command]
pub fn activity_custom_save(db: State<Mutex<Connection>>, activity: Activity) -> DbResult<Activity> {
    save_custom(&*lock(&db)?, &activity)
}

#[tauri::command]
pub fn activity_custom_delete(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    delete_custom(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn activity_recommend(db: State<Mutex<Connection>>, input: RecommendInput) -> DbResult<Vec<Recommendation>> {
    recommend(&*lock(&db)?, &input)
}

#[cfg(test)]
mod tests;
