//! 평어 데이터 파일(템플릿, 금지 표현)과 교육과정 데이터를 앱에 포함해 읽는다.
//! 파일 내용은 `npm run validate:data`와 이 모듈의 테스트가 검증한다.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

use crate::db::DbResult;

const TEMPLATES_JSON: &str = include_str!("../../../../data/evaluation/templates.json");
const FORBIDDEN_JSON: &str = include_str!("../../../../data/evaluation/forbidden.json");
const CURRICULUM_JSON: &str = include_str!("../../../../data/curriculum/integrated-2022.json");

pub const SUBJECTS: [&str; 3] = ["바", "슬", "즐"];

pub fn check_subject(subject: &str) -> DbResult<&'static str> {
    SUBJECTS.iter().copied().find(|s| *s == subject).ok_or_else(|| "교과는 바른 생활·슬기로운 생활·즐거운 생활 중에서 골라 주세요".to_string())
}

// ---------- 템플릿 ----------

#[derive(Debug, Deserialize)]
pub struct Endings {
    pub ham: String,
    pub eum: String,
    pub boim: String,
}

#[derive(Debug, Deserialize)]
pub struct Template {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub group: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Templates {
    pub endings: Endings,
    /// 교과 -> (활동 태그 -> 템플릿 묶음)
    pub tag_groups: HashMap<String, HashMap<String, String>>,
    pub templates: Vec<Template>,
}

pub fn templates() -> &'static Templates {
    static T: OnceLock<Templates> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(TEMPLATES_JSON).expect("data/evaluation/templates.json"))
}

// ---------- 금지 표현 ----------

#[derive(Debug, Deserialize)]
struct RawEntry {
    id: String,
    category: String,
    kind: String,
    pattern: String,
    #[serde(default)]
    except: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawForbidden {
    categories: HashMap<String, String>,
    entries: Vec<RawEntry>,
}

#[derive(Debug)]
pub struct ForbiddenEntry {
    pub label: String,
    pattern: String,
    re: Option<Regex>,
    except: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForbiddenHit {
    /// 예: "서열·비교 표현"
    pub label: String,
    /// 문장에서 걸린 부분.
    pub matched: String,
}

#[derive(Debug)]
pub struct Forbidden {
    entries: Vec<ForbiddenEntry>,
}

pub const EXTRA_LABEL: &str = "교사가 더한 금지 표현";

impl Forbidden {
    fn parse(json: &str) -> Result<Forbidden, String> {
        let raw: RawForbidden = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        for e in raw.entries {
            let re = if e.kind == "regex" { Some(Regex::new(&e.pattern).map_err(|err| format!("{}: {err}", e.id))?) } else { None };
            let label = raw.categories.get(&e.category).cloned().unwrap_or(e.category);
            entries.push(ForbiddenEntry { label, pattern: e.pattern, re, except: e.except });
        }
        Ok(Forbidden { entries })
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[ForbiddenEntry] {
        &self.entries
    }

    /// 문장에서 걸린 금지 표현(같은 부분은 한 번만). `extra`는 교사가 더한 낱말.
    pub fn find(&self, text: &str, extra: &[String]) -> Vec<ForbiddenHit> {
        let mut hits: Vec<ForbiddenHit> = Vec::new();
        let mut push = |label: &str, matched: &str| {
            if !hits.iter().any(|h| h.matched == matched) {
                hits.push(ForbiddenHit { label: label.to_string(), matched: matched.to_string() });
            }
        };
        for e in &self.entries {
            // 예외 낱말(예: 가장자리) 안의 일치는 세지 않는다.
            let masked = e.except.iter().fold(text.to_string(), |acc, x| acc.replace(x.as_str(), "\u{0}"));
            match &e.re {
                Some(re) => {
                    if let Some(m) = re.find(&masked) {
                        push(&e.label, m.as_str());
                    }
                }
                None => {
                    if masked.contains(e.pattern.as_str()) {
                        push(&e.label, &e.pattern);
                    }
                }
            }
        }
        for w in extra.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
            if text.contains(w) {
                push(EXTRA_LABEL, w);
            }
        }
        hits
    }
}

pub fn forbidden() -> &'static Forbidden {
    static F: OnceLock<Forbidden> = OnceLock::new();
    F.get_or_init(|| Forbidden::parse(FORBIDDEN_JSON).expect("data/evaluation/forbidden.json"))
}

// ---------- 교육과정 ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicBookInfo {
    pub id: String,
    pub title: String,
    pub area: i64,
    pub standard_index: i64,
}

#[derive(Debug, Deserialize)]
pub struct StandardInfo {
    pub code: String,
    pub subject: String,
    pub statement: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurriculumInfo {
    pub topic_books: Vec<TopicBookInfo>,
    pub standards: Vec<StandardInfo>,
}

pub fn curriculum() -> &'static CurriculumInfo {
    static C: OnceLock<CurriculumInfo> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(CURRICULUM_JSON).expect("data/curriculum/integrated-2022.json"))
}

pub fn topic_title(id: Option<&str>) -> Option<String> {
    let id = id?;
    curriculum().topic_books.iter().find(|t| t.id == id).map(|t| t.title.clone())
}

/// 성취기준 코드의 교과(`[2슬02-04]` -> 슬). 데이터에 없으면 코드 모양에서 읽는다.
pub fn subject_of_code(code: &str) -> Option<String> {
    if let Some(s) = curriculum().standards.iter().find(|s| s.code == code) {
        return Some(s.subject.clone());
    }
    code.strip_prefix('[').and_then(|r| r.chars().nth(1)).map(|c| c.to_string())
}

/// 교과 이름(바 -> 바른 생활).
pub fn curriculum_subject_name(code: &str) -> &'static str {
    match code {
        "바" => "바른 생활",
        "슬" => "슬기로운 생활",
        _ => "즐거운 생활",
    }
}

pub fn standard_statement(code: &str) -> Option<String> {
    curriculum().standards.iter().find(|s| s.code == code).and_then(|s| s.statement.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_files_parse() {
        assert!(templates().templates.len() >= 60);
        assert!(forbidden().entries().len() >= 40);
        assert_eq!(subject_of_code("[2슬02-04]").as_deref(), Some("슬"));
        assert_eq!(topic_title(Some("g1s1-exploration")).as_deref(), Some("탐험"));
    }

    #[test]
    fn forbidden_finds_each_category_and_honours_exceptions() {
        let f = forbidden();
        let labels = |t: &str| f.find(t, &[]).into_iter().map(|h| h.label).collect::<Vec<_>>();
        assert_eq!(labels("반에서 가장 빠르게 달림"), ["서열·비교 표현"]);
        assert_eq!(labels("1등을 하였음"), ["서열·비교 표현"]);
        assert_eq!(labels("100점을 받음"), ["서열·비교 표현"]);
        assert_eq!(labels("학원에서 미리 배워 알고 있음"), ["사교육·외부 기관 표현"]);
        assert_eq!(labels("아버지가 의사여서 알고 있음"), ["가족 직업·경제 표현"]);
        assert_eq!(labels("맞벌이 가정이라 늦음"), ["가족 직업·경제 표현"]);
        assert_eq!(labels("수업 중 산만하여"), ["낙인 표현"]);
        // 예외 낱말
        assert!(labels("운동장 가장자리를 따라 걸음").is_empty());
        assert!(labels("장애물 넘기 놀이에 참여함").is_empty());
        assert!(labels("친구를 도와주며 협력함").is_empty());
    }

    #[test]
    fn outside_personal_and_exception_entries() {
        let f = forbidden();
        let labels = |t: &str| f.find(t, &[]).into_iter().map(|h| h.label).collect::<Vec<_>>();
        assert_eq!(labels("교외 대회에서 수상한 경험을 이야기함"), ["사교육·외부 기관 표현"]);
        assert_eq!(labels("외부 상장을 받았다고 말함"), ["사교육·외부 기관 표현"]);
        assert_eq!(labels("한글 자격증이 있다고 말함"), ["사교육·외부 기관 표현"]);
        assert_eq!(labels("종교 행사에 다녀옴"), ["종교·개인 신상"]);
        assert!(labels("인공지능 로봇을 관찰함").is_empty());
        assert_eq!(labels("지능이 낮아 보임"), ["낙인 표현"]);
    }

    #[test]
    fn teacher_extra_words_are_flagged() {
        let hits = forbidden().find("해바라기반 활동에 참여함", &["해바라기반".to_string(), "  ".to_string()]);
        assert_eq!(hits, [ForbiddenHit { label: EXTRA_LABEL.to_string(), matched: "해바라기반".to_string() }]);
    }
}
