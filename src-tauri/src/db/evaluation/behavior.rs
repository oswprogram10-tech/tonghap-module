//! 행동특성 및 종합의견 초안. 설계: `docs/design/24-behavior-opinion.md`.
//!
//! 교과 평어(`bundle`·`generate`·`check`)와 같은 틀을 따르되, 한 아이의 기간 안 관찰 **전체**(교과 무관)에서 근거를 모은다.
//! 「지도가 필요한 모습」 기록은 `negative`로 표시되고, 쓸 때는 반드시 변화 가능성을 함께 써야 한다.
//! 모든 문장은 선생님이 「근거 확인」을 체크해야 확정된다(AI가 만든 문장을 그대로 입력하지 않도록 하는 장치).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::bundle::{first_chars, habit_cands, pick_balanced, Bundle, BundlePeriod, BundleStudent, Cand, Evidence, ALIAS, MIN_EVIDENCE};
use super::check::{check_draft, total_chars, Warning};
use super::combined::{ends_with_noun_form, find_symbols, neis_bytes, split_sentences, NOUN_ENDING, SYMBOL};
use super::data::{behavior_templates, forbidden, topic_title, Forbidden};
use super::generate::{fill, hash, joined_text, note_phrase, Sentence, Style};
use super::{is_unchanged_from_model, load_settings, normalize_sentences_max, other_names, StudentRow, ViewStudent};
use crate::db::reports::{check_period, Scrubber};
use crate::db::{classes, new_id, now_iso, observations, settings, students, DbExt, DbResult};

/// 근거 묶음의 `subject` 값(교과가 아니라는 표시).
pub const SUBJECT: &str = "행";
pub const MAX_EVIDENCE: usize = 14;
const MAX_HABITS: usize = 2;
const MAX_PAIRS: usize = 3;
pub const MAX_SENTENCES: usize = 24;
const MAX_TEMPLATE_SENTENCES: usize = 8;

pub const KEY_TARGET: &str = "behaviorTargetChars";
pub const DEFAULT_TARGET: usize = 300;
pub const TARGET_MIN: usize = 150;
pub const TARGET_MAX: usize = 800;
/// 이보다 짧으면 「너무 짧습니다」 경고를 낸다(초등은 글자 수 제한이 없다).
const SHORT_CHARS: usize = 60;

pub const NO_CHANGE: &str = "noChange";
pub const NO_STRENGTH: &str = "noStrength";

// ---------- 설정 ----------

pub fn load_target(conn: &Connection) -> DbResult<usize> {
    Ok(settings::get(conn, KEY_TARGET)?.as_u64().map(|n| n as usize).filter(|n| (TARGET_MIN..=TARGET_MAX).contains(n)).unwrap_or(DEFAULT_TARGET))
}

pub fn save_target(conn: &Connection, target: usize) -> DbResult<usize> {
    if !(TARGET_MIN..=TARGET_MAX).contains(&target) {
        return Err(format!("목표 글자 수는 {TARGET_MIN}~{TARGET_MAX}자 사이로 정해 주세요"));
    }
    settings::set(conn, KEY_TARGET, &serde_json::json!(target))?;
    load_target(conn)
}

// ---------- 학기와 인성·생활 태그 ----------

/// 3~8월은 1학기, 나머지는 2학기.
pub fn semester_of(date: &str) -> u8 {
    match date.get(5..7).and_then(|m| m.parse::<u32>().ok()) {
        Some(3..=8) => 1,
        _ => 2,
    }
}

fn is_life_tag(tag: &str) -> bool {
    behavior_templates().tag_groups.contains_key(tag)
}

pub fn semester_label(sem: u8) -> &'static str {
    &behavior_templates().semester_labels[usize::from(sem == 2)]
}

pub fn strip_semester_labels(text: &str) -> String {
    behavior_templates().semester_labels.iter().fold(text.to_string(), |acc, l| acc.replace(l.as_str(), " "))
}

/// 모델이 멋대로 쓴 학기 표시를 지운다(표시는 앱이 덩어리 앞에만 붙인다).
pub fn remove_semester_labels(text: &str) -> String {
    strip_semester_labels(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------- 근거 묶음 ----------

struct Obs {
    cand: Cand,
    tags: Vec<String>,
    negative: bool,
}

/// 앞선 「지도가 필요한 모습」마다, 그 뒤에 같은 태그를 가진 첫 긍정 기록을 짝으로 묶는다(최대 3쌍).
/// 돌려주는 값은 (짝이 되는 긍정 기록의 위치, 앞선 기록의 위치).
fn find_pairs(obs: &[Obs]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, neg) in obs.iter().enumerate().filter(|(_, o)| o.negative) {
        if pairs.len() >= MAX_PAIRS {
            break;
        }
        let partner = obs.iter().enumerate().skip(i + 1).find(|(j, o)| {
            !o.negative && !pairs.iter().any(|(p, _)| p == j) && o.tags.iter().any(|t| neg.tags.contains(t))
        });
        if let Some((j, _)) = partner {
            pairs.push((j, i));
        }
    }
    pairs
}

fn observation_cands(conn: &Connection, media: &Path, class_id: &str, student_id: &str, from: &str, to: &str, sc: &Scrubber) -> DbResult<Vec<Obs>> {
    let mut stmt = conn
        .prepare(
            "SELECT o.id FROM observations o
             JOIN observation_students os ON os.observation_id = o.id AND os.student_id = ?1
             WHERE o.class_id = ?2 AND o.deleted_at IS NULL AND substr(o.observed_at, 1, 10) BETWEEN ?3 AND ?4
             ORDER BY o.observed_at, o.id",
        )
        .db()?;
    let ids: Vec<String> = stmt.query_map(params![student_id, class_id, from, to], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    let mut out = Vec::new();
    for id in ids {
        let o = observations::get(conn, media, &id)?;
        let text = sc.scrub_own_to(o.note.trim(), ALIAS).trim().to_string();
        let quote = o
            .attachments
            .iter()
            .filter(|a| a.kind == "audio" && a.transcript_status == "done")
            .filter_map(|a| a.transcript.as_deref())
            .map(|t| sc.scrub_own_to(t.trim(), ALIAS).trim().to_string())
            .find(|t| !t.is_empty())
            .map(|t| first_chars(&t, 80));
        if text.is_empty() && quote.is_none() {
            continue;
        }
        let tags: Vec<String> = o.tags.iter().map(|t| sc.scrub(t)).collect();
        out.push(Obs {
            negative: o.needs_guidance,
            tags: tags.clone(),
            cand: Cand {
                group: 0,
                topic_id: o.topic_book_id.clone(),
                ev: Evidence::Observation {
                    id: format!("o:{}", o.id),
                    date: first_chars(&o.observed_at, 10),
                    topic: topic_title(o.topic_book_id.as_deref()),
                    standards: vec![],
                    tags,
                    text,
                    quote,
                    together: o.students.iter().any(|s| s.id != student_id),
                    negative: o.needs_guidance,
                    pair: None,
                },
            },
        });
    }
    Ok(out)
}

/// 근거 묶음의 짝 표시 중 짝이 묶음에 없는 것을 지운다(근거를 뺐을 때).
pub fn prune_pairs(bundle: &mut Bundle) {
    let ids: Vec<String> = bundle.evidence.iter().map(|e| e.id().to_string()).collect();
    for e in bundle.evidence.iter_mut() {
        if let Evidence::Observation { pair, .. } = e {
            if pair.as_ref().is_some_and(|p| !ids.contains(p)) {
                *pair = None;
            }
        }
    }
}

/// 한 아이의 기간 전체 관찰에서 행동특성 근거 묶음을 만든다(교과와 무관).
/// 우선순위: 짝 근거 -> 인성·생활 태그 -> 짝 없는 「지도가 필요한 모습」 -> 그 밖. 최대 14개(습관 2개 포함), 주제책이 고르게 섞인다.
pub fn build_bundle(conn: &Connection, media: &Path, student_id: &str, from: &str, to: &str, pinned: &[String]) -> DbResult<Bundle> {
    let (from, to, _) = check_period(from, to)?;
    let student = students::get(conn, student_id)?;
    let class = classes::get(conn, &student.class_id)?;
    let sc = Scrubber::new(conn, &class.id, student_id)?;

    let mut obs = observation_cands(conn, media, &class.id, student_id, &from, &to, &sc)?;
    let pairs = find_pairs(&obs);
    for (pos, neg) in &pairs {
        let neg_id = obs[*neg].cand.ev.id().to_string();
        if let Evidence::Observation { pair, .. } = &mut obs[*pos].cand.ev {
            *pair = Some(neg_id);
        }
    }
    for (i, o) in obs.iter_mut().enumerate() {
        let paired = pairs.iter().any(|(p, n)| *p == i || *n == i);
        o.cand.group = if paired {
            0
        } else if o.negative {
            2
        } else if o.tags.iter().any(|t| is_life_tag(t)) {
            1
        } else {
            3
        };
    }
    let habits = habit_cands(conn, student_id, &from, &to, &sc)?;
    let habit_n = habits.len().min(MAX_HABITS);
    let obs_cap = MAX_EVIDENCE - habit_n;

    let cands: Vec<Cand> = obs.into_iter().map(|o| o.cand).collect();
    let mut picked: Vec<Evidence> = Vec::new();
    for i in pick_balanced(&habits, habit_n) {
        picked.push(habits[i].ev.clone());
    }
    for i in pick_balanced(&cands, obs_cap) {
        picked.push(cands[i].ev.clone());
    }
    for c in habits.iter().chain(cands.iter()) {
        if pinned.iter().any(|p| p == c.ev.id()) && !picked.iter().any(|e| e.id() == c.ev.id()) {
            picked.push(c.ev.clone());
        }
    }
    picked.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let mut bundle = Bundle {
        student: BundleStudent { alias: ALIAS.to_string() },
        subject: SUBJECT.to_string(),
        period: BundlePeriod { from, to },
        evidence: picked,
        standards_text: BTreeMap::new(),
    };
    prune_pairs(&mut bundle);
    Ok(bundle)
}

// ---------- 템플릿 생성기 ----------

/// 한 학기(또는 전체) 몫의 근거로 만들 문장 덩어리.
fn chunk_in(e: &Evidence, sem: Option<u8>) -> bool {
    match (sem, e) {
        (None, _) => true,
        (Some(s), Evidence::Observation { date, .. }) => semester_of(date) == s,
        // 습관 근거는 날짜가 없으므로 학기를 나눌 때는 2학기 몫으로 본다.
        (Some(s), Evidence::Habit { .. }) => s == 2,
        (Some(_), Evidence::Session { .. }) => false,
    }
}

struct Item {
    kind: &'static str,
    group: String,
    evidence: Vec<String>,
    before: Option<String>,
    note: Option<String>,
    topic: Option<String>,
}

fn clean(s: Option<String>, f: &Forbidden, extra: &[String]) -> Option<String> {
    s.filter(|v| f.find(v, extra).is_empty())
}

/// (id, 주제책, 태그, 메모, 짝) 한 관찰 근거의 부분들.
type ObsParts<'a> = (&'a str, &'a Option<String>, &'a Vec<String>, &'a str, Option<&'a String>);

fn obs_parts(e: &Evidence) -> Option<ObsParts<'_>> {
    match e {
        Evidence::Observation { id, topic, tags, text, pair, .. } => Some((id, topic, tags, text, pair.as_ref())),
        _ => None,
    }
}

fn candidate_templates<'a>(item: &Item, style: Style) -> Vec<&'a super::data::BehaviorTemplate> {
    let all = &behavior_templates().templates;
    let usable = |group: &str| -> Vec<&'a super::data::BehaviorTemplate> {
        all.iter()
            .filter(|t| t.kind == item.kind && t.group == group)
            .filter(|t| {
                super::generate::placeholders(&t.text).iter().all(|(n, ..)| match n.as_str() {
                    "note" => item.note.is_some(),
                    "before" => item.before.is_some(),
                    "topic" => item.topic.is_some(),
                    _ => false,
                })
            })
            .filter(|t| style != Style::Boim || !t.text.contains("모습"))
            .collect()
    };
    let mut list = usable(&item.group);
    if list.is_empty() && item.group != "general" {
        list = usable("general");
    }
    list
}

fn render(item: &Item, style: Style, seed: &str, used: &mut Vec<String>) -> Option<Sentence> {
    let mut cands = candidate_templates(item, style);
    if cands.iter().any(|t| !used.contains(&t.id)) {
        cands.retain(|t| !used.contains(&t.id));
    }
    if cands.is_empty() {
        return None;
    }
    let t = cands[(hash(&[seed, &item.evidence.join(","), "행"]) % cands.len() as u64) as usize];
    let get = |name: &str| match name {
        "note" => item.note.clone(),
        "before" => item.before.clone(),
        "topic" => item.topic.clone(),
        _ => None,
    };
    used.push(t.id.clone());
    Some(Sentence { text: fill(&t.text, style.ending(), &get), evidence: item.evidence.clone(), reviewed: false })
}

/// 한 덩어리(전체 또는 한 학기)의 문장: 장점 -> 성장·변화(짝) -> 지도가 필요한 모습(변화 가능성과 함께).
#[allow(clippy::too_many_arguments)]
fn chunk_sentences(bundle: &Bundle, sem: Option<u8>, style: Style, target: usize, seed: &str, f: &Forbidden, extra: &[String], used: &mut Vec<String>) -> Vec<Sentence> {
    let in_chunk: Vec<&Evidence> = bundle.evidence.iter().filter(|e| chunk_in(e, sem)).collect();
    let find = |id: &str| bundle.evidence.iter().find(|e| e.id() == id);
    let mut pair_items: Vec<Item> = Vec::new();
    let mut strengths: Vec<Item> = Vec::new();
    let mut concerns: Vec<Item> = Vec::new();
    let mut used_negatives: Vec<String> = Vec::new();

    for e in &in_chunk {
        let Some((id, topic, tags, text, pair)) = obs_parts(e) else { continue };
        if e.is_negative() {
            continue;
        }
        let note = clean(note_phrase(text), f, extra);
        let group = tags.iter().find_map(|t| behavior_templates().tag_groups.get(t)).cloned().unwrap_or_else(|| "general".to_string());
        if let Some(neg) = pair.and_then(|p| find(p)) {
            if let Some((nid, _, _, ntext, _)) = obs_parts(neg) {
                let before = clean(note_phrase(ntext), f, extra);
                let item = Item { kind: "growth", group: "general".into(), evidence: vec![nid.to_string(), id.to_string()], before, note: note.clone(), topic: topic.clone() };
                if before_and_note_ok(&item) && !candidate_templates(&item, style).is_empty() {
                    used_negatives.push(nid.to_string());
                    pair_items.push(item);
                    continue;
                }
            }
        }
        strengths.push(Item { kind: "strength", group, evidence: vec![id.to_string()], before: None, note, topic: topic.clone() });
    }
    for e in &in_chunk {
        let Some((id, _, _, text, _)) = obs_parts(e) else { continue };
        if e.is_negative() && !used_negatives.iter().any(|u| u == id) {
            concerns.push(Item { kind: "concern", group: "general".into(), evidence: vec![id.to_string()], before: None, note: clean(note_phrase(text), f, extra), topic: None });
        }
    }

    // 인성·생활 태그가 있는 장점을 먼저, 시드로 시작 위치를 돌려 학생마다 다른 기록이 앞에 오게 한다.
    strengths.sort_by_key(|s| s.group == "general");
    let life = strengths.iter().filter(|s| s.group != "general").count();
    if life > 1 {
        let k = (hash(&[seed, "행-start"]) % life as u64) as usize;
        strengths[..life].rotate_left(k);
    }

    pair_items.truncate(2);
    let mut reserved: Vec<Sentence> = Vec::new();
    for it in &pair_items {
        reserved.extend(render(it, style, seed, used));
    }
    let mut chosen_strengths: Vec<Sentence> = Vec::new();
    let reserved_len: usize = reserved.iter().map(|s| s.text.chars().count() + 1).sum();
    let mut running = reserved_len;
    for it in &strengths {
        if chosen_strengths.len() + reserved.len() >= MAX_TEMPLATE_SENTENCES - 1 || (!chosen_strengths.is_empty() && running + 40 >= target) {
            break;
        }
        if let Some(s) = render(it, style, seed, used) {
            running += s.text.chars().count() + 1;
            chosen_strengths.push(s);
        }
    }
    let mut out = chosen_strengths;
    out.extend(reserved);
    // 장점·성장 문장이 하나도 없으면 단점만 쓰지 않는다.
    if !out.is_empty() {
        for it in &concerns {
            if let Some(s) = render(it, style, seed, used) {
                out.push(s);
                break;
            }
        }
    }
    out
}

fn before_and_note_ok(item: &Item) -> bool {
    item.before.is_some() && item.note.is_some()
}

/// 템플릿으로 문장을 만든다. `split`이 참이면 1학기·2학기 근거를 따로 묶어 만들고 각 덩어리 앞에 「(1학기)」「(2학기)」를 붙인다.
pub fn generate_template(bundle: &Bundle, style: Style, target: usize, seed: &str, split: bool, extra: &[String]) -> Vec<Sentence> {
    let f = forbidden();
    let mut used: Vec<String> = Vec::new();
    if !split {
        return chunk_sentences(bundle, None, style, target, seed, f, extra, &mut used);
    }
    let mut out: Vec<Sentence> = Vec::new();
    for sem in [1u8, 2] {
        let mut chunk = chunk_sentences(bundle, Some(sem), style, target / 2, seed, f, extra, &mut used);
        if let Some(first) = chunk.first_mut() {
            first.text = prefixed(semester_label(sem), &first.text);
        }
        out.extend(chunk);
    }
    out
}

/// 문장 앞에 학기 표시를 붙인다(이미 붙어 있으면 그대로).
pub fn prefixed(label: &str, text: &str) -> String {
    if text.trim_start().starts_with(label) {
        text.to_string()
    } else {
        format!("{label} {}", text.trim_start())
    }
}

// ---------- 검사 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorCheck {
    /// 문장마다의 경고(문장 순서와 같다).
    pub sentences: Vec<Vec<Warning>>,
    /// 글 전체에 대한 경고(단점만 있고 장점·성장이 없음 등).
    pub overall: Vec<Warning>,
    pub total_chars: usize,
    pub total_bytes: usize,
    pub length: Option<Warning>,
    /// 문장이 있고 모든 문장이 묶음에 있는 근거를 하나 이상 가리킨다.
    pub can_confirm: bool,
    pub all_reviewed: bool,
}

fn warn(code: &str, message: String) -> Warning {
    Warning { code: code.to_string(), message }
}

fn tail10(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(10);
    let t: String = chars[start..].iter().collect();
    if start > 0 {
        format!("…{t}")
    } else {
        t
    }
}

pub fn has_change_word(text: &str) -> bool {
    behavior_templates().change_words.iter().any(|w| text.contains(w.as_str()))
}

/// 설계 08의 검사(금지 표현, 다른 학생 이름, 근거 없음, 근거와의 어긋남)에 명사형 종결, 특수문자·번호,
/// 단점 문장의 변화 가능성, 장점 없음을 더한다. 학기 표시 「(1학기)」「(2학기)」는 특수문자 검사에서 뺀다.
pub fn check_behavior(sentences: &[Sentence], bundle: &Bundle, extra: &[String], other: &[String]) -> BehaviorCheck {
    let base = check_draft(sentences, bundle, forbidden(), extra, other);
    let mut per = base.sentences;
    let is_negative = |id: &String| bundle.evidence.iter().any(|e| e.id() == id && e.is_negative());
    for (i, s) in sentences.iter().enumerate() {
        let text = strip_semester_labels(&s.text);
        if let Some(bad) = split_sentences(&text).into_iter().find(|p| !ends_with_noun_form(p)) {
            per[i].push(warn(NOUN_ENDING, format!("명사형 어미(~함, ~음)로 끝내 주세요: '{}'", tail10(&bad))));
        }
        let symbols = find_symbols(&text);
        if !symbols.is_empty() {
            per[i].push(warn(SYMBOL, format!("특수문자·번호 기호는 쓰지 않는 것이 좋습니다: {}", symbols.iter().map(|x| format!("'{x}'")).collect::<Vec<_>>().join(" "))));
        }
        if s.evidence.iter().any(is_negative) && !has_change_word(&s.text) {
            per[i].push(warn(NO_CHANGE, "단점을 쓸 때는 변화 가능성을 함께 써 주세요(기재요령)".to_string()));
        }
    }
    let mut overall = Vec::new();
    if !sentences.is_empty() && sentences.iter().all(|s| s.evidence.iter().any(is_negative)) {
        overall.push(warn(NO_STRENGTH, "단점 문장만 있습니다. 장점이나 성장·변화 문장을 함께 써 주세요".to_string()));
    }
    let total = total_chars(sentences);
    let text = joined_text(sentences);
    let length = (!sentences.is_empty() && total < SHORT_CHARS).then(|| warn("length", format!("너무 짧습니다({total}자)")));
    BehaviorCheck {
        sentences: per,
        overall,
        total_chars: total,
        total_bytes: neis_bytes(&text),
        length,
        can_confirm: base.can_confirm,
        all_reviewed: base.all_reviewed,
    }
}

// ---------- 저장 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredOpinion {
    pub id: String,
    pub sentences: Vec<Sentence>,
    pub split_semesters: bool,
    /// draft | confirmed
    pub status: String,
    /// template | manual | llm:<모델 id>
    pub generator: String,
    pub confirmed_at: Option<String>,
    pub updated_at: String,
    /// 언어 모델이 만든 글을 한 글자도 고치지 않았다(확정 전에 화면이 한 번 더 묻는다).
    pub unchanged_from_model: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpinionKey {
    pub student_id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateInput {
    #[serde(flatten)]
    pub key: OpinionKey,
    /// 교사가 체크를 풀어 뺀 근거 id.
    #[serde(default)]
    pub excluded_ids: Vec<String>,
    #[serde(default)]
    pub split_semesters: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveInput {
    #[serde(flatten)]
    pub key: OpinionKey,
    pub sentences: Vec<Sentence>,
    #[serde(default)]
    pub split_semesters: bool,
}

/// 정규화된 (학생 id, 시작, 끝).
pub(crate) struct Key {
    pub student_id: String,
    pub from: String,
    pub to: String,
}

pub(crate) fn norm(key: &OpinionKey) -> DbResult<Key> {
    let (from, to, _) = check_period(&key.from, &key.to)?;
    Ok(Key { student_id: key.student_id.clone(), from, to })
}

pub fn load_opinion(conn: &Connection, k: &Key) -> DbResult<Option<StoredOpinion>> {
    #[allow(clippy::type_complexity)]
    let row: Option<(String, String, i64, String, String, Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT id, sentences, split_semesters, status, generator, confirmed_at, updated_at, generated_text FROM behavior_opinions
             WHERE student_id = ?1 AND period_from = ?2 AND period_to = ?3",
            params![k.student_id, k.from, k.to],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
        )
        .optional()
        .db()?;
    row.map(|(id, sentences, split, status, generator, confirmed_at, updated_at, generated)| {
        let mut sentences: Vec<Sentence> = serde_json::from_str(&sentences).map_err(|e| format!("저장된 행동특성 및 종합의견을 읽을 수 없습니다: {e}"))?;
        if status == "confirmed" {
            sentences.iter_mut().for_each(|s| s.reviewed = true);
        }
        let unchanged_from_model = is_unchanged_from_model(&generator, generated.as_deref(), &joined_text(&sentences));
        Ok(StoredOpinion { id, sentences, split_semesters: split != 0, status, generator, confirmed_at, updated_at, unchanged_from_model })
    })
    .transpose()
}

fn write_opinion(conn: &Connection, k: &Key, split: bool, sentences: &[Sentence], generator: &str) -> DbResult<()> {
    let student = students::get(conn, &k.student_id)?;
    let json = serde_json::to_string(sentences).expect("JSON");
    let now = now_iso();
    let generated = generator.starts_with("llm:").then(|| joined_text(sentences));
    conn.execute(
        "INSERT INTO behavior_opinions (id, class_id, student_id, period_from, period_to, split_semesters, sentences, status, generator, created_at, updated_at, generated_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'draft', ?8, ?9, ?9, ?10)
         ON CONFLICT (student_id, period_from, period_to) DO UPDATE SET
           sentences = excluded.sentences, split_semesters = excluded.split_semesters, status = 'draft', confirmed_at = NULL, updated_at = excluded.updated_at,
           generator = CASE WHEN ?8 = 'manual' THEN generator ELSE excluded.generator END,
           generated_text = CASE WHEN ?8 = 'manual' THEN generated_text ELSE excluded.generated_text END",
        params![new_id(), student.class_id, student.id, k.from, k.to, i64::from(split), json, generator, now, generated],
    )
    .db()?;
    Ok(())
}

pub(crate) fn write_generated(conn: &Connection, k: &Key, split: bool, sentences: &[Sentence], generator: &str) -> DbResult<()> {
    write_opinion(conn, k, split, sentences, generator)
}

fn evidence_ids(sentences: &[Sentence]) -> Vec<String> {
    sentences.iter().flat_map(|s| s.evidence.iter().cloned()).collect()
}

// ---------- 화면용 묶음 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorView {
    pub student: ViewStudent,
    pub from: String,
    pub to: String,
    pub bundle: Bundle,
    pub enough: bool,
    pub min_evidence: usize,
    pub target_chars: usize,
    pub opinion: Option<StoredOpinion>,
    pub check: Option<BehaviorCheck>,
}

fn run_check(conn: &Connection, student_id: &str, sentences: &[Sentence], bundle: &Bundle) -> DbResult<BehaviorCheck> {
    let s = load_settings(conn)?;
    Ok(check_behavior(sentences, bundle, &s.extra_forbidden, &other_names(conn, student_id)?))
}

pub fn view(conn: &Connection, media: &Path, key: &OpinionKey) -> DbResult<BehaviorView> {
    let k = norm(key)?;
    let student = students::get(conn, &k.student_id)?;
    let opinion = load_opinion(conn, &k)?;
    let pinned = opinion.as_ref().map(|d| evidence_ids(&d.sentences)).unwrap_or_default();
    let bundle = build_bundle(conn, media, &k.student_id, &k.from, &k.to, &pinned)?;
    let check = opinion.as_ref().map(|d| run_check(conn, &k.student_id, &d.sentences, &bundle)).transpose()?;
    Ok(BehaviorView {
        student: ViewStudent { id: student.id, number: student.number, name: student.name },
        enough: bundle.evidence.len() >= MIN_EVIDENCE,
        min_evidence: MIN_EVIDENCE,
        target_chars: load_target(conn)?,
        from: k.from,
        to: k.to,
        bundle,
        opinion,
        check,
    })
}

pub fn insufficient_message(n: usize) -> String {
    format!("기록이 부족합니다({n}개). 이 기간의 관찰 기록을 더 남겨 주세요")
}

/// 체크를 푼 근거를 빼고 짝 표시를 정리한 근거 묶음. 3개 미만이면 오류.
pub(crate) fn generation_bundle(conn: &Connection, media: &Path, k: &Key, excluded: &[String]) -> DbResult<Bundle> {
    let mut b = build_bundle(conn, media, &k.student_id, &k.from, &k.to, &[])?;
    b.evidence.retain(|e| !excluded.iter().any(|x| x == e.id()));
    prune_pairs(&mut b);
    if b.evidence.len() < MIN_EVIDENCE {
        return Err(insufficient_message(b.evidence.len()));
    }
    Ok(b)
}

/// 템플릿으로 초안을 만들어 저장한다. 근거가 3개 미만이면 만들지 않는다.
pub fn generate_draft(conn: &Connection, media: &Path, input: &GenerateInput) -> DbResult<BehaviorView> {
    let k = norm(&input.key)?;
    let b = generation_bundle(conn, media, &k, &input.excluded_ids)?;
    let s = load_settings(conn)?;
    let style = Style::parse(&s.style).unwrap_or(Style::Ham);
    let sentences = generate_template(&b, style, load_target(conn)?, &k.student_id, input.split_semesters, &s.extra_forbidden);
    if sentences.is_empty() {
        return Err("초안을 만들 수 없습니다. 근거를 더 골라 주세요(장점이나 성장으로 쓸 기록이 필요합니다)".to_string());
    }
    write_opinion(conn, &k, input.split_semesters, &sentences, "template")?;
    view(conn, media, &input.key)
}

/// 교사가 고친 문장을 저장한다. 확정은 풀리고 초안이 된다.
pub fn save_draft(conn: &Connection, media: &Path, input: &SaveInput) -> DbResult<BehaviorView> {
    let k = norm(&input.key)?;
    let sentences = normalize_sentences_max(&input.sentences, MAX_SENTENCES)?;
    write_opinion(conn, &k, input.split_semesters, &sentences, "manual")?;
    view(conn, media, &input.key)
}

/// 저장하지 않고 문장만 검사한다(편집 중 경고 갱신).
pub fn check_sentences(conn: &Connection, media: &Path, input: &SaveInput) -> DbResult<BehaviorCheck> {
    let k = norm(&input.key)?;
    let sentences: Vec<Sentence> = input
        .sentences
        .iter()
        .take(MAX_SENTENCES)
        .map(|s| Sentence { text: s.text.trim().to_string(), evidence: s.evidence.clone(), reviewed: s.reviewed })
        .collect();
    let bundle = build_bundle(conn, media, &k.student_id, &k.from, &k.to, &evidence_ids(&sentences))?;
    run_check(conn, &k.student_id, &sentences, &bundle)
}

/// 확정한다. 모든 문장에 근거가 있고 「근거 확인」이 체크되어야 한다.
pub fn confirm(conn: &Connection, media: &Path, key: &OpinionKey) -> DbResult<BehaviorView> {
    let k = norm(key)?;
    let o = load_opinion(conn, &k)?.ok_or("확정할 초안이 없습니다. 먼저 초안을 만들거나 문장을 저장해 주세요")?;
    if o.sentences.is_empty() {
        return Err("문장이 없어 확정할 수 없습니다".to_string());
    }
    let bundle = build_bundle(conn, media, &k.student_id, &k.from, &k.to, &evidence_ids(&o.sentences))?;
    for (i, s) in o.sentences.iter().enumerate() {
        let ok = !s.evidence.is_empty() && s.evidence.iter().all(|id| bundle.evidence.iter().any(|e| e.id() == id));
        if !ok {
            return Err(format!("{}번째 문장에 근거가 없습니다. 문장마다 근거를 하나 이상 붙여야 확정할 수 있습니다", i + 1));
        }
    }
    if let Some(i) = o.sentences.iter().position(|s| !s.reviewed) {
        return Err(format!("{}번째 문장의 「근거 확인」을 체크해 주세요. AI나 템플릿이 만든 문장은 근거와 실제 모습을 확인한 뒤 확정할 수 있습니다", i + 1));
    }
    conn.execute("UPDATE behavior_opinions SET status = 'confirmed', confirmed_at = ?2 WHERE id = ?1", params![o.id, now_iso()]).db()?;
    view(conn, media, key)
}

// ---------- 학급 현황과 내보내기 ----------

pub fn class_status(conn: &Connection, media: &Path, class_id: &str, from: &str, to: &str) -> DbResult<Vec<StudentRow>> {
    let (from, to, _) = check_period(from, to)?;
    classes::get(conn, class_id)?;
    let mut stmt = conn.prepare("SELECT student_id, status FROM behavior_opinions WHERE class_id = ?1 AND period_from = ?2 AND period_to = ?3").db()?;
    let statuses: HashMap<String, String> = stmt.query_map(params![class_id, from, to], |r| Ok((r.get(0)?, r.get(1)?))).db()?.collect::<Result<_, _>>().db()?;
    let mut rows = Vec::new();
    for s in students::list(conn, class_id, false)? {
        let count = build_bundle(conn, media, &s.id, &from, &to, &[])?.evidence.len();
        let status = statuses.get(&s.id).cloned().unwrap_or_else(|| "none".to_string());
        rows.push(StudentRow { student_id: s.id, number: s.number, name: s.name, evidence_count: count, status });
    }
    Ok(rows)
}

/// 학급의 확정본만 모은 텍스트(번호, 이름, 행동특성 및 종합의견을 탭으로 나눔). 확정본이 없으면 오류.
pub fn export_text(conn: &Connection, class_id: &str, from: &str, to: &str) -> DbResult<(String, usize)> {
    let (from, to, _) = check_period(from, to)?;
    classes::get(conn, class_id)?;
    let mut lines = vec!["번호\t이름\t행동특성 및 종합의견".to_string()];
    let mut count = 0;
    for s in students::list(conn, class_id, false)? {
        let k = Key { student_id: s.id.clone(), from: from.clone(), to: to.clone() };
        let Some(o) = load_opinion(conn, &k)?.filter(|o| o.status == "confirmed") else { continue };
        let name = s.name.split_whitespace().collect::<Vec<_>>().join(" ");
        lines.push(format!("{}\t{}\t{}", s.number, name, joined_text(&o.sentences)));
        count += 1;
    }
    if count == 0 {
        return Err("확정된 행동특성 및 종합의견이 없습니다. 학생마다 「확정」을 누른 뒤 내보내 주세요".to_string());
    }
    Ok((lines.join("\r\n") + "\r\n", count))
}
