//! (1) 근거 모으기: 한 아이의 한 교과 기록을 근거 묶음(JSON)으로 모은다.
//!
//! 이 묶음은 템플릿 생성기와(나중에) 로컬 언어 모델이 똑같이 입력으로 받는다.
//! 개인정보: 학생은 `"학생"`으로 바꾸고, 메모·자막·차시 제목에 적힌 다른 학생 이름은 "친구"로 바꾼다.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::data::{check_subject, standard_statement, subject_of_code, templates, topic_title};
use crate::db::reports::{check_period, parse_date, Scrubber};
use crate::db::{classes, habits, observations, students, DbExt, DbResult};

pub const ALIAS: &str = "학생";
pub const MIN_EVIDENCE: usize = 3;
pub const MAX_EVIDENCE: usize = 12;
const MAX_HABITS: usize = 2;
const MAX_SESSIONS: usize = 3;
const QUOTE_CHARS: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleStudent {
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundlePeriod {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Evidence {
    Observation {
        id: String,
        date: String,
        topic: Option<String>,
        /// 이 교과의 성취기준만.
        standards: Vec<String>,
        tags: Vec<String>,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        quote: Option<String>,
        together: bool,
    },
    Habit {
        id: String,
        title: String,
        done_days: i64,
        school_days: i64,
    },
    Session {
        id: String,
        date: String,
        topic: Option<String>,
        title: String,
        standards: Vec<String>,
    },
}

impl Evidence {
    pub fn id(&self) -> &str {
        match self {
            Evidence::Observation { id, .. } | Evidence::Habit { id, .. } | Evidence::Session { id, .. } => id,
        }
    }

    fn sort_key(&self) -> (u8, &str, &str) {
        match self {
            Evidence::Habit { id, .. } => (0, "", id),
            Evidence::Observation { id, date, .. } | Evidence::Session { id, date, .. } => (1, date, id),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    pub student: BundleStudent,
    pub subject: String,
    pub period: BundlePeriod,
    pub evidence: Vec<Evidence>,
    pub standards_text: BTreeMap<String, String>,
}

/// 고르기 전의 후보. `group`이 작을수록 먼저, `topic_id`로 주제책을 고르게 섞는다.
struct Cand {
    ev: Evidence,
    group: u8,
    topic_id: Option<String>,
}

/// 그 교과의 성격 태그(활동 태그 중 템플릿 묶음에 대응하는 것).
pub fn subject_tags(subject: &str) -> Vec<&'static str> {
    templates().tag_groups.get(subject).map(|m| m.keys().map(String::as_str).collect()).unwrap_or_default()
}

fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn subject_standards(codes: &[String], subject: &str) -> Vec<String> {
    codes.iter().filter(|c| subject_of_code(c).as_deref() == Some(subject)).cloned().collect()
}

#[allow(clippy::too_many_arguments)]
fn observation_cands(conn: &Connection, media: &Path, class_id: &str, student_id: &str, subject: &str, from: &str, to: &str, sc: &Scrubber) -> DbResult<Vec<Cand>> {
    let mut stmt = conn
        .prepare(
            "SELECT o.id FROM observations o
             JOIN observation_students os ON os.observation_id = o.id AND os.student_id = ?1
             WHERE o.class_id = ?2 AND o.deleted_at IS NULL AND substr(o.observed_at, 1, 10) BETWEEN ?3 AND ?4
             ORDER BY o.observed_at, o.id",
        )
        .db()?;
    let ids: Vec<String> = stmt.query_map(params![student_id, class_id, from, to], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    let tags_of_subject = subject_tags(subject);
    let mut out = Vec::new();
    for id in ids {
        let o = observations::get(conn, media, &id)?;
        let standards = subject_standards(&o.standard_codes, subject);
        let group = if !standards.is_empty() {
            0
        } else if o.tags.iter().any(|t| tags_of_subject.contains(&t.as_str())) {
            1
        } else {
            2
        };
        let text = sc.scrub_own_to(o.note.trim(), ALIAS).trim().to_string();
        // 교사가 확인한(고친 것 포함) 자막만. 자막 상태가 done이 아니면 넣지 않는다.
        let quote = o
            .attachments
            .iter()
            .filter(|a| a.kind == "audio" && a.transcript_status == "done")
            .filter_map(|a| a.transcript.as_deref())
            .map(|t| sc.scrub_own_to(t.trim(), ALIAS).trim().to_string())
            .find(|t| !t.is_empty())
            .map(|t| first_chars(&t, QUOTE_CHARS));
        if text.is_empty() && quote.is_none() {
            continue; // 가리킬 내용이 없는 기록
        }
        out.push(Cand {
            group,
            topic_id: o.topic_book_id.clone(),
            ev: Evidence::Observation {
                id: format!("o:{}", o.id),
                date: first_chars(&o.observed_at, 10),
                topic: topic_title(o.topic_book_id.as_deref()),
                standards,
                tags: o.tags.iter().map(|t| sc.scrub(t)).collect(),
                text,
                quote,
                together: o.students.iter().any(|s| s.id != student_id),
            },
        });
    }
    Ok(out)
}

fn session_cands(conn: &Connection, class_id: &str, subject: &str, from: &str, to: &str, sc: &Scrubber) -> DbResult<Vec<Cand>> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, COALESCE(s.scheduled_on, substr(s.updated_at, 1, 10)) AS d, u.topic_book_id
             FROM unit_sessions s JOIN units u ON u.id = s.unit_id
             WHERE u.class_id = ?1 AND s.status = 'done' AND COALESCE(s.scheduled_on, substr(s.updated_at, 1, 10)) BETWEEN ?2 AND ?3
             ORDER BY d, s.id",
        )
        .db()?;
    let rows: Vec<(String, String, String, Option<String>)> =
        stmt.query_map(params![class_id, from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).db()?.collect::<Result<_, _>>().db()?;
    let mut out = Vec::new();
    for (id, title, date, topic) in rows {
        let mut st = conn.prepare("SELECT standard_code FROM unit_session_standards WHERE session_id = ?1 ORDER BY standard_code").db()?;
        let codes: Vec<String> = st.query_map([&id], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
        let standards = subject_standards(&codes, subject);
        if standards.is_empty() {
            continue;
        }
        out.push(Cand {
            group: 0,
            topic_id: topic.clone(),
            ev: Evidence::Session { id: format!("s:{id}"), date, topic: topic_title(topic.as_deref()), title: sc.scrub_own_to(title.trim(), ALIAS), standards },
        });
    }
    Ok(out)
}

fn habit_cands(conn: &Connection, student_id: &str, from: &str, to: &str, sc: &Scrubber) -> DbResult<Vec<Cand>> {
    let days = (parse_date(to)? - parse_date(from)?).num_days() + 1;
    let summary = habits::student_summary(conn, student_id, to, days)?;
    Ok(summary
        .into_iter()
        .filter(|h| h.school_days > 0)
        .map(|h| Cand {
            group: 0,
            topic_id: None,
            ev: Evidence::Habit { id: format!("h:{}", h.habit_id), title: sc.scrub_own_to(h.title.trim(), ALIAS), done_days: h.done_days, school_days: h.school_days },
        })
        .collect())
}

/// 우선순위 묶음(group) 순서대로, 묶음 안에서는 주제책을 번갈아 가며 `cap`개까지 고른다.
fn pick_balanced(cands: &[Cand], cap: usize) -> Vec<usize> {
    let mut chosen = Vec::new();
    for group in 0..=2u8 {
        let mut buckets: Vec<(Option<&str>, VecDeque<usize>)> = Vec::new();
        for (i, c) in cands.iter().enumerate().filter(|(_, c)| c.group == group) {
            let key = c.topic_id.as_deref();
            match buckets.iter_mut().find(|(k, _)| *k == key) {
                Some((_, q)) => q.push_back(i),
                None => buckets.push((key, VecDeque::from([i]))),
            }
        }
        while chosen.len() < cap && buckets.iter().any(|(_, q)| !q.is_empty()) {
            for (_, q) in buckets.iter_mut() {
                if chosen.len() >= cap {
                    break;
                }
                if let Some(i) = q.pop_front() {
                    chosen.push(i);
                }
            }
        }
    }
    chosen
}

/// 근거 묶음을 만든다. `pinned`는 이미 문장이 가리키는 근거 id로, 후보에 있으면 개수 제한과 상관없이 넣는다.
pub fn build(conn: &Connection, media: &Path, student_id: &str, subject: &str, from: &str, to: &str, pinned: &[String]) -> DbResult<Bundle> {
    let subject = check_subject(subject)?;
    let (from, to, _) = check_period(from, to)?;
    let student = students::get(conn, student_id)?;
    let class = classes::get(conn, &student.class_id)?;
    let sc = Scrubber::new(conn, &class.id, student_id)?;

    let obs = observation_cands(conn, media, &class.id, student_id, subject, &from, &to, &sc)?;
    let sessions = session_cands(conn, &class.id, subject, &from, &to, &sc)?;
    let habit = if subject == "바" { habit_cands(conn, student_id, &from, &to, &sc)? } else { vec![] };

    let habit_n = habit.len().min(MAX_HABITS);
    let session_n = sessions.len().min(MAX_SESSIONS);
    let obs_cap = MAX_EVIDENCE - habit_n - session_n;

    let mut picked: Vec<Evidence> = Vec::new();
    for i in pick_balanced(&habit, habit_n) {
        picked.push(habit[i].ev.clone());
    }
    for i in pick_balanced(&obs, obs_cap) {
        picked.push(obs[i].ev.clone());
    }
    for i in pick_balanced(&sessions, session_n) {
        picked.push(sessions[i].ev.clone());
    }
    for c in habit.iter().chain(obs.iter()).chain(sessions.iter()) {
        if pinned.iter().any(|p| p == c.ev.id()) && !picked.iter().any(|e| e.id() == c.ev.id()) {
            picked.push(c.ev.clone());
        }
    }
    picked.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    let mut standards_text = BTreeMap::new();
    for e in &picked {
        let codes = match e {
            Evidence::Observation { standards, .. } | Evidence::Session { standards, .. } => standards.as_slice(),
            Evidence::Habit { .. } => &[],
        };
        for c in codes {
            if let Some(t) = standard_statement(c) {
                standards_text.insert(c.clone(), t);
            }
        }
    }
    Ok(Bundle { student: BundleStudent { alias: ALIAS.to_string() }, subject: subject.to_string(), period: BundlePeriod { from, to }, evidence: picked, standards_text })
}
