//! (3) 자동 검사: 문장마다 경고를 붙인다. 차단이 아니라 표시이며 교사가 판단한다.
//! 단, 확정은 모든 문장에 근거가 있어야 한다(`DraftCheck::can_confirm`).

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Serialize;

use super::bundle::{Bundle, Evidence};
use super::data::{curriculum, templates, Forbidden};
use super::generate::Sentence;

/// 평어 전체 글자 수 허용 범위(설정의 목표 글자 수도 이 안에서 고른다).
pub const LEN_MIN: usize = 150;
pub const LEN_MAX: usize = 400;

pub const NO_EVIDENCE: &str = "noEvidence";
pub const FORBIDDEN: &str = "forbidden";
pub const OTHER_NAME: &str = "otherName";
pub const MISMATCH: &str = "mismatch";
pub const LENGTH: &str = "length";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

fn warn(code: &str, message: String) -> Warning {
    Warning { code: code.to_string(), message }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DraftCheck {
    /// 문장마다의 경고(문장 순서와 같다).
    pub sentences: Vec<Vec<Warning>>,
    pub total_chars: usize,
    pub length: Option<Warning>,
    /// 문장이 하나 이상이고, 모든 문장이 글자가 있고 묶음에 있는 근거를 하나 이상 가리킨다.
    pub can_confirm: bool,
}

pub fn total_chars(sentences: &[Sentence]) -> usize {
    sentences.iter().map(|s| s.text.trim().chars().count()).sum::<usize>() + sentences.len().saturating_sub(1)
}

/// 문장에서 꺼낸 낱말(2글자 이상). 조사가 붙은 꼴도 비교하도록 뒤 1~2글자를 뗀 꼴도 함께 쓴다.
pub(crate) fn variants(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    let mut out = vec![token.to_string()];
    for cut in 1..=2 {
        if chars.len() > cut + 1 {
            out.push(chars[..chars.len() - cut].iter().collect());
        }
    }
    out
}

pub(crate) fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|t| t.chars().count() >= 2).map(str::to_string).collect()
}

/// 템플릿에 쓰인 낱말(틀 자체의 말투)은 근거와 비교하지 않는다.
fn boilerplate() -> &'static HashSet<String> {
    static B: OnceLock<HashSet<String>> = OnceLock::new();
    B.get_or_init(|| {
        let mut set = HashSet::new();
        for t in &templates().templates {
            let mut text = t.text.clone();
            while let (Some(a), Some(b)) = (text.find('{'), text.find('}')) {
                if a >= b {
                    break;
                }
                text.replace_range(a..=b, " ");
            }
            for tok in tokens(&text) {
                set.extend(variants(&tok));
            }
        }
        // 교과 성취기준·주제책 이름이 아닌 일반 낱말만 남기려고 주제책 이름은 빼 둔다.
        for tb in &curriculum().topic_books {
            set.remove(&tb.title);
        }
        set
    })
}

fn evidence_text(e: &Evidence, bundle: &Bundle) -> String {
    let mut parts: Vec<String> = Vec::new();
    match e {
        Evidence::Observation { topic, standards, tags, text, quote, .. } => {
            parts.extend(topic.clone());
            parts.extend(standards.iter().filter_map(|c| bundle.standards_text.get(c).cloned()));
            parts.extend(tags.clone());
            parts.push(text.clone());
            parts.extend(quote.clone());
        }
        Evidence::Habit { title, .. } => parts.push(title.clone()),
        Evidence::Session { topic, title, standards, .. } => {
            parts.extend(topic.clone());
            parts.extend(standards.iter().filter_map(|c| bundle.standards_text.get(c).cloned()));
            parts.push(title.clone());
        }
    }
    parts.join(" ")
}

/// 문장의 낱말이 가리킨 근거 어디에도 나오지 않으면(겹침 0) 참. 비교할 낱말이 없으면 거짓.
fn no_overlap(sentence: &str, haystack: &str) -> bool {
    let stop = boilerplate();
    let content: Vec<String> = tokens(sentence).into_iter().filter(|t| !variants(t).iter().any(|v| stop.contains(v))).collect();
    !content.is_empty() && !content.iter().any(|t| variants(t).iter().any(|v| haystack.contains(v.as_str())))
}

/// 초안 검사. `other_names`는 학급의 다른 학생 이름(두 글자 이상).
pub fn check_draft(sentences: &[Sentence], bundle: &Bundle, forbidden: &Forbidden, extra: &[String], other_names: &[String]) -> DraftCheck {
    let mut per = Vec::new();
    let mut can_confirm = !sentences.is_empty();
    for s in sentences {
        let mut w = Vec::new();
        let valid: Vec<&Evidence> = s.evidence.iter().filter_map(|id| bundle.evidence.iter().find(|e| e.id() == id)).collect();
        if s.text.trim().is_empty() || valid.is_empty() {
            can_confirm = false;
        }
        if s.evidence.is_empty() {
            w.push(warn(NO_EVIDENCE, "근거가 없습니다. 이 문장이 바탕으로 한 기록을 붙여 주세요".to_string()));
        } else if valid.len() < s.evidence.len() {
            w.push(warn(NO_EVIDENCE, "근거 묶음에 없는 기록을 가리킵니다. 기록이 지워졌거나 기간이 다를 수 있어요".to_string()));
        }
        for h in forbidden.find(&s.text, extra) {
            w.push(warn(FORBIDDEN, format!("금지 표현({}): '{}'", h.label, h.matched)));
        }
        let mut names: Vec<&String> = other_names.iter().filter(|n| n.chars().count() >= 2 && s.text.contains(n.as_str())).collect();
        names.dedup();
        for n in names {
            w.push(warn(OTHER_NAME, format!("다른 학생 이름이 들어 있습니다: '{n}'")));
        }
        if !valid.is_empty() {
            let haystack = valid.iter().map(|e| evidence_text(e, bundle)).collect::<Vec<_>>().join(" ");
            if no_overlap(&s.text, &haystack) {
                w.push(warn(MISMATCH, "근거와 맞는지 확인해 주세요. 문장의 주제나 활동 낱말이 가리킨 기록에 없습니다".to_string()));
            }
        }
        per.push(w);
    }
    let total = total_chars(sentences);
    let length = if sentences.is_empty() {
        None
    } else if total < LEN_MIN {
        Some(warn(LENGTH, format!("너무 짧습니다({total}자). {LEN_MIN}~{LEN_MAX}자를 권합니다")))
    } else if total > LEN_MAX {
        Some(warn(LENGTH, format!("너무 깁니다({total}자). {LEN_MIN}~{LEN_MAX}자를 권합니다")))
    } else {
        None
    };
    DraftCheck { sentences: per, total_chars: total, length, can_confirm }
}
