//! (2) 템플릿 생성기: 근거 묶음 -> 문장 목록(문장마다 근거 id).
//!
//! 언어 모델 생성기가 붙어도 같은 입력(`Bundle`)과 같은 출력(`Sentence`)을 쓴다.
//! 같은 입력과 같은 시드(학생 id)면 항상 같은 초안이 나온다.

use serde::{Deserialize, Serialize};

use super::bundle::{Bundle, Evidence};
use super::data::{templates, Forbidden, Template};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sentence {
    pub text: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Ham,
    Eum,
    Boim,
}

impl Style {
    pub fn parse(s: &str) -> Option<Style> {
        match s {
            "ham" => Some(Style::Ham),
            "eum" => Some(Style::Eum),
            "boim" => Some(Style::Boim),
            _ => None,
        }
    }

    fn ending(self) -> &'static str {
        let e = &templates().endings;
        match self {
            Style::Ham => &e.ham,
            Style::Eum => &e.eum,
            Style::Boim => &e.boim,
        }
    }
}

const MAX_SENTENCES: usize = 4;
const MIN_SENTENCES: usize = 2;
const NOTE_MAX_CHARS: usize = 40;
const QUOTE_MAX_CHARS: usize = 40;

/// 시드 문자열에서 안정적인 숫자를 만든다(FNV-1a; 표준 라이브러리 해시는 버전마다 달라질 수 있다).
pub(crate) fn hash(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0xff)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

// ---------- 한국어 보조 ----------

fn is_hangul(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

/// 받침 번호(0=없음, 8=ㄹ, 16=ㅁ). 한글이 아니면 받침 없음으로 본다.
fn final_index(c: char) -> u32 {
    if is_hangul(c) {
        (c as u32 - 0xAC00) % 28
    } else {
        0
    }
}

/// `을/를`, `은/는`, `이/가`, `과/와`, `으로/로` 중 알맞은 조사.
pub fn josa(word: &str, pair: &str) -> &'static str {
    let (with_final, without) = pair.split_once('/').unwrap_or((pair, pair));
    let f = word.trim_end().chars().last().map_or(0, final_index);
    let pick = if with_final == "으로" { f != 0 && f != 8 } else { f != 0 };
    let chosen = if pick { with_final } else { without };
    match chosen {
        "을" => "을",
        "를" => "를",
        "은" => "은",
        "는" => "는",
        "이" => "이",
        "가" => "가",
        "과" => "과",
        "와" => "와",
        "으로" => "으로",
        "로" => "로",
        _ => "",
    }
}

/// 교사 메모 한 문장을 "…는/…던"(뒤에 " 모습"이 붙는 꼴)으로 바꾼다. 바꿀 수 없으면 `None`.
/// 예: "친구를 도와줌" -> "친구를 도와주는", "질문하였음" -> "질문하였던".
pub fn note_phrase(memo: &str) -> Option<String> {
    let first = memo.split(['.', '!', '?', '\n']).map(str::trim).find(|s| !s.is_empty())?;
    let s = first.trim_matches(|c: char| matches!(c, '\'' | '"' | '‘' | '’' | '“' | '”' | ',' | '~' | ' '));
    let past_char = |c: char| matches!(c, '았' | '었' | '였' | '했') || final_index(c) == 20;
    // 말투를 "~음"/"~함" 꼴로 먼저 맞춘다: 했습니다/먹습니다/합니다/했어요/먹어요
    let mut norm: Vec<char> = s.chars().collect();
    let tail = |v: &[char], t: &str| v.ends_with(&t.chars().collect::<Vec<_>>());
    if tail(&norm, "습니다") {
        norm.truncate(norm.len() - 3);
        norm.push('음');
    } else if tail(&norm, "니다") && norm.len() >= 3 && final_index(norm[norm.len() - 3]) == 17 {
        let c = norm[norm.len() - 3];
        norm.truncate(norm.len() - 3);
        norm.push(char::from_u32(c as u32 - 1)?); // ㅂ받침 -> ㅁ받침(합 -> 함)
    } else if tail(&norm, "어요") || tail(&norm, "아요") || tail(&norm, "여요") {
        norm.truncate(norm.len() - 2);
        norm.push('음');
    }
    let chars = norm;
    if chars.len() < 3 || chars.len() > NOTE_MAX_CHARS || s.contains("모습") || s.contains(['{', '}']) {
        return None;
    }
    let last = *chars.last()?;
    let prev = chars.len().checked_sub(2).map(|i| chars[i]);
    let stem: String = chars[..chars.len() - 1].iter().collect();
    let past = |c: Option<char>| c.is_some_and(past_char);
    let out = if last == '음' {
        // 했음/주었음 -> 했던/주었던, 찾음/있음 -> 찾는/있는
        if past(prev) {
            format!("{stem}던")
        } else {
            format!("{stem}는")
        }
    } else if last == '다' {
        if past(prev) {
            format!("{stem}던")
        } else if prev == Some('는') || prev.is_some_and(|p| final_index(p) == 4) {
            stem // 먹는다 -> 먹는, 도와준다 -> 도와준
        } else {
            return None;
        }
    } else if final_index(last) == 16 {
        // 함 -> 하는, 줌 -> 주는, 봄 -> 보는
        let base = char::from_u32(last as u32 - 16)?;
        format!("{stem}{base}는")
    } else {
        return None;
    };
    Some(out)
}

/// 자막 인용에서 첫 문장(40자 이하)을 꺼낸다. 물음표·느낌표는 남긴다.
pub fn quote_phrase(quote: &str) -> Option<String> {
    let mut sentence = String::new();
    for c in quote.trim().chars() {
        sentence.push(c);
        if matches!(c, '.' | '?' | '!' | '\n') {
            break;
        }
    }
    let cleaned: String = sentence.chars().filter(|c| !matches!(c, '\'' | '"' | '‘' | '’' | '“' | '”' | '{' | '}' | '\n')).collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ',']).trim().to_string();
    let n = cleaned.chars().count();
    ((2..=QUOTE_MAX_CHARS).contains(&n) && !cleaned.contains("모습")).then_some(cleaned)
}

// ---------- 슬롯 ----------

#[derive(Default)]
struct Slots {
    topic: Option<String>,
    note: Option<String>,
    quote: Option<String>,
    title: Option<String>,
}

impl Slots {
    fn get(&self, name: &str) -> Option<&str> {
        match name {
            "topic" => self.topic.as_deref(),
            "note" => self.note.as_deref(),
            "quote" => self.quote.as_deref(),
            "title" => self.title.as_deref(),
            _ => None,
        }
    }
}

/// `{슬롯}` 또는 `{슬롯|을/를}`에서 (이름, 조사쌍, 시작, 끝)을 차례로 꺼낸다. `{E}`는 건너뛴다.
fn placeholders(text: &str) -> Vec<(String, Option<String>, usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find('{') {
        let start = from + rel;
        let Some(len) = text[start..].find('}') else { break };
        let end = start + len + 1;
        let inner = &text[start + 1..end - 1];
        if inner != "E" {
            let (name, pair) = match inner.split_once('|') {
                Some((n, p)) => (n.to_string(), Some(p.to_string())),
                None => (inner.to_string(), None),
            };
            out.push((name, pair, start, end));
        }
        from = end;
    }
    out
}

fn needs(t: &Template) -> Vec<String> {
    placeholders(&t.text).into_iter().map(|(n, ..)| n).collect()
}

fn render(t: &Template, slots: &Slots, ending: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (name, pair, start, end) in placeholders(&t.text) {
        out.push_str(&t.text[last..start]);
        let value = slots.get(&name).unwrap_or("");
        out.push_str(value);
        if let Some(p) = pair {
            out.push_str(josa(value, &p));
        }
        last = end;
    }
    out.push_str(&t.text[last..]);
    let mut s = out.replace("{E}", ending);
    s.push('.');
    s
}

/// 습관 실천율 구간.
fn habit_band(done: i64, total: i64) -> &'static str {
    let rate = if total > 0 { done as f64 / total as f64 } else { 0.0 };
    if rate >= 0.8 {
        "high"
    } else if rate >= 0.5 {
        "mid"
    } else {
        "low"
    }
}

struct Item {
    kind: &'static str,
    group: String,
    slots: Slots,
}

fn item_of(ev: &Evidence, subject: &str, forbidden: &Forbidden, extra: &[String]) -> Item {
    let clean = |s: Option<String>| s.filter(|v| forbidden.find(v, extra).is_empty());
    match ev {
        Evidence::Observation { topic, tags, text, quote, .. } => {
            let groups = templates().tag_groups.get(subject);
            let group = tags.iter().find_map(|t| groups.and_then(|g| g.get(t))).cloned().unwrap_or_else(|| "general".to_string());
            Item {
                kind: "observation",
                group,
                slots: Slots { topic: topic.clone(), note: clean(note_phrase(text)), quote: clean(quote.as_deref().and_then(quote_phrase)), title: None },
            }
        }
        Evidence::Habit { title, done_days, school_days, .. } => Item {
            kind: "habit",
            group: habit_band(*done_days, *school_days).to_string(),
            slots: Slots { title: Some(title.replace('\'', "")), ..Slots::default() },
        },
        Evidence::Session { topic, title, .. } => {
            Item { kind: "session", group: "default".to_string(), slots: Slots { topic: topic.clone(), title: Some(title.replace('\'', "")), ..Slots::default() } }
        }
    }
}

/// 이 근거에 쓸 수 있는 템플릿. 근거에서 값을 얻은 슬롯이 하나라도 있는 틀을 먼저 쓰고,
/// 없으면 슬롯 없는 일반 틀을 쓴다(문장이 근거 내용과 이어지도록).
fn candidates<'a>(item: &Item, subject: &str, style: Style) -> Vec<&'a Template> {
    let all = &templates().templates;
    let usable = |group: &str| -> Vec<&'a Template> {
        all.iter()
            .filter(|t| t.kind == item.kind && t.subject == subject && t.group == group)
            .filter(|t| needs(t).iter().all(|n| item.slots.get(n).is_some()))
            // "~하는 모습을 보임" 문체에서 "모습"이 두 번 나오지 않게 한다.
            .filter(|t| style != Style::Boim || !t.text.contains("모습"))
            .collect()
    };
    let mut list = usable(&item.group);
    if list.is_empty() && item.group != "general" && item.kind == "observation" {
        list = usable("general");
    }
    let grounded: Vec<&Template> = list.iter().copied().filter(|t| !needs(t).is_empty()).collect();
    if grounded.is_empty() {
        list
    } else {
        grounded
    }
}

/// 문장으로 만들 근거의 순서: 습관 하나, 그다음 관찰(주제책이 다른 것 먼저)과 차시를 번갈아.
fn order<'a>(bundle: &'a Bundle, seed: &str) -> Vec<&'a Evidence> {
    let habits: Vec<&Evidence> = bundle.evidence.iter().filter(|e| matches!(e, Evidence::Habit { .. })).collect();
    let mut obs: Vec<&Evidence> = bundle.evidence.iter().filter(|e| matches!(e, Evidence::Observation { .. })).collect();
    let sessions: Vec<&Evidence> = bundle.evidence.iter().filter(|e| matches!(e, Evidence::Session { .. })).collect();
    // 시드로 시작 위치만 돌려 학생마다 다른 기록이 앞에 오게 한다(같은 입력이면 같은 결과).
    if !obs.is_empty() {
        let k = (hash(&[seed, &bundle.subject]) % obs.len() as u64) as usize;
        obs.rotate_left(k);
    }
    // 같은 주제책이 연달아 나오지 않도록 새 주제책을 먼저 고른다.
    let topic_of = |e: &Evidence| match e {
        Evidence::Observation { topic, .. } | Evidence::Session { topic, .. } => topic.clone(),
        Evidence::Habit { .. } => None,
    };
    let mut seen: Vec<Option<String>> = Vec::new();
    let (mut fresh, mut rest) = (Vec::new(), Vec::new());
    for e in obs {
        let t = topic_of(e);
        if seen.contains(&t) {
            rest.push(e);
        } else {
            seen.push(t);
            fresh.push(e);
        }
    }
    fresh.extend(rest);

    let mut out: Vec<&Evidence> = habits.into_iter().take(1).collect();
    let mut sessions = sessions.into_iter();
    for o in fresh {
        out.push(o);
        // 관찰마다 차시를 하나씩 끼워 넣어 다양하게 한다.
        out.extend(sessions.next());
    }
    out.extend(sessions);
    out
}

/// 문장 2~4개를 만든다. 전체 길이가 `target_chars`에 가까워지면 멈춘다.
pub fn generate(bundle: &Bundle, style: Style, target_chars: usize, seed: &str, forbidden: &Forbidden, extra: &[String]) -> Vec<Sentence> {
    let ending = style.ending();
    let mut sentences: Vec<Sentence> = Vec::new();
    let mut used: Vec<&str> = Vec::new();
    let mut total = 0usize;
    for ev in order(bundle, seed) {
        if sentences.len() >= MAX_SENTENCES || (sentences.len() >= MIN_SENTENCES && total + 20 >= target_chars) {
            break;
        }
        let item = item_of(ev, &bundle.subject, forbidden, extra);
        let mut cands = candidates(&item, &bundle.subject, style);
        if cands.iter().any(|t| !used.contains(&t.id.as_str())) {
            cands.retain(|t| !used.contains(&t.id.as_str()));
        }
        if cands.is_empty() {
            continue;
        }
        let t = cands[(hash(&[seed, ev.id(), &bundle.subject]) % cands.len() as u64) as usize];
        let text = render(t, &item.slots, ending);
        used.push(&t.id);
        total += text.chars().count() + usize::from(!sentences.is_empty());
        sentences.push(Sentence { text, evidence: vec![ev.id().to_string()] });
    }
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn josa_follows_final_consonant() {
        assert_eq!(josa("학교", "을/를"), "를");
        assert_eq!(josa("탐험", "을/를"), "을");
        assert_eq!(josa("마을", "으로/로"), "로");
        assert_eq!(josa("사람들", "은/는"), "은");
        assert_eq!(josa("친구", "이/가"), "가");
        assert_eq!(josa("ABC", "을/를"), "를");
    }

    #[test]
    fn note_phrase_turns_memos_into_modifiers() {
        assert_eq!(note_phrase("친구를 도와줌").as_deref(), Some("친구를 도와주는"));
        assert_eq!(note_phrase("질문함.").as_deref(), Some("질문하는"));
        assert_eq!(note_phrase("블록을 정리해 주었음").as_deref(), Some("블록을 정리해 주었던"));
        assert_eq!(note_phrase("끝까지 찾아봄").as_deref(), Some("끝까지 찾아보는"));
        assert_eq!(note_phrase("책을 읽는다").as_deref(), Some("책을 읽는"));
        assert_eq!(note_phrase("처음 말한 문장함. 둘째 문장함").as_deref(), Some("처음 말한 문장하는"));
        assert_eq!(note_phrase("도와주었어요").as_deref(), Some("도와주었던"));
        assert_eq!(note_phrase("블록을 쌓았습니다").as_deref(), Some("블록을 쌓았던"));
        assert_eq!(note_phrase("책을 읽습니다").as_deref(), Some("책을 읽는"));
        assert_eq!(note_phrase("친구와 정리합니다").as_deref(), Some("친구와 정리하는"));
        assert_eq!(note_phrase("혼자 찾아요").as_deref(), Some("혼자 찾는"));
        assert_eq!(note_phrase("친구를 도와줬어요").as_deref(), Some("친구를 도와줬던"));
        assert_eq!(note_phrase("열심히 모습을 보임"), None);
        assert_eq!(note_phrase("짧"), None);
        assert_eq!(note_phrase(&"가".repeat(60)), None);
    }

    #[test]
    fn quote_phrase_takes_first_short_sentence() {
        assert_eq!(quote_phrase("별은 왜 반짝여요? 낮에도 있어요?").as_deref(), Some("별은 왜 반짝여요?"));
        assert_eq!(quote_phrase("'우와' 신기하다.").as_deref(), Some("우와 신기하다"));
        assert_eq!(quote_phrase(&"가".repeat(60)), None);
    }

    #[test]
    fn every_template_renders_in_every_style_without_leftovers() {
        let slots = Slots { topic: Some("학교".into()), note: Some("친구를 도와주는".into()), quote: Some("왜 그래요?".into()), title: Some("교실 정리".into()) };
        for t in &templates().templates {
            for style in [Style::Ham, Style::Eum, Style::Boim] {
                let s = render(t, &slots, style.ending());
                assert!(!s.contains('{') && !s.contains('}'), "{}: {s}", t.id);
                assert!(s.ends_with(&format!("{}.", style.ending())), "{}: {s}", t.id);
            }
        }
    }
}
