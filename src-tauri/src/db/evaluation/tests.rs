use std::path::PathBuf;

use rusqlite::Connection;

use super::bundle::{self, Bundle, Evidence};
use super::check::{check_draft, FORBIDDEN, LENGTH, MISMATCH, NO_EVIDENCE, OTHER_NAME};
use super::data::{forbidden, templates};
use super::generate::{generate, Sentence, Style};
use super::*;
use crate::db::observations::{create, ObservationInput};
use crate::db::{attachments, observations, test_conn};

const FROM: &str = "2027-03-02";
const TO: &str = "2027-07-18";

struct Cx {
    conn: Connection,
    media: PathBuf,
    class: String,
    /// 김하늘, 박서준, 이지우
    ids: Vec<String>,
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tonghap-eval-{}-{}", name, new_id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn seed() -> Cx {
    let mut conn = test_conn();
    let c = classes::create(&conn, 2027, 1, "1반").unwrap();
    students::import(&mut conn, &c.id, "1 김하늘\n2 박서준\n3 이지우").unwrap();
    let ids = students::list(&conn, &c.id, true).unwrap().into_iter().map(|s| s.id).collect();
    Cx { conn, media: tmp("media"), class: c.id, ids }
}

fn key(cx: &Cx, i: usize, subject: &str) -> DraftKey {
    DraftKey { student_id: cx.ids[i].clone(), subject: subject.to_string(), from: FROM.to_string(), to: TO.to_string() }
}

#[allow(clippy::too_many_arguments)]
fn obs_full(cx: &mut Cx, students: &[usize], day: &str, topic: &str, note: &str, codes: &[&str], tags: &[&str]) -> observations::Observation {
    let input = ObservationInput {
        class_id: cx.class.clone(),
        observed_at: format!("{day}T10:00:00+09:00"),
        topic_book_id: Some(topic.to_string()),
        note: note.to_string(),
        student_ids: students.iter().map(|i| cx.ids[*i].clone()).collect(),
        standard_codes: codes.iter().map(|c| c.to_string()).collect(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        unit_session_id: None,
    };
    create(&mut cx.conn, &cx.media, &input).unwrap()
}

fn obs(cx: &mut Cx, day: &str, note: &str) -> observations::Observation {
    obs_full(cx, &[0], day, "g1s1-exploration", note, &["[2슬01-01]"], &["탐구"])
}

/// 슬 교과 기록 n개(주제책 둘을 번갈아).
fn seed_inquiry(cx: &mut Cx, n: usize) -> Vec<String> {
    let notes = ["우주 영상을 보고 궁금한 점을 질문함", "돋보기로 곤충을 자세히 관찰함", "교실 둘레를 살펴보고 새로 알게 된 점을 이야기함", "개미가 이동하는 길을 끝까지 따라가 봄"];
    (0..n)
        .map(|i| {
            let topic = if i % 2 == 0 { "g1s1-exploration" } else { "g1s1-school" };
            obs_full(cx, &[0], &format!("2027-04-{:02}", i + 1), topic, notes[i % notes.len()], &["[2슬01-01]"], &["탐구"]).id
        })
        .collect()
}

fn bundle_of(cx: &Cx, i: usize, subject: &str) -> Bundle {
    bundle::build(&cx.conn, &cx.media, &cx.ids[i], subject, FROM, TO, &[]).unwrap()
}

fn sentence(text: &str, ev: &[&str]) -> Sentence {
    Sentence { text: text.to_string(), evidence: ev.iter().map(|e| e.to_string()).collect() }
}

fn codes_of(w: &[check::Warning]) -> Vec<&str> {
    w.iter().map(|x| x.code.as_str()).collect()
}

// ---------- (1) 근거 묶음 ----------

#[test]
fn bundle_uses_alias_and_never_contains_other_students_names() {
    let mut cx = seed();
    let o = obs_full(
        &mut cx,
        &[0, 1],
        "2027-04-10",
        "g1s1-exploration",
        "김하늘이 박서준이랑 함께 별을 관찰함. 이지우가 도와줌",
        &["[2슬01-01]"],
        &["탐구", "질문"],
    );
    let src = tmp("src").join("a.webm");
    std::fs::write(&src, b"audio").unwrap();
    let au = attachments::add_bytes(&cx.conn, &cx.media, &o.id, "audio", "audio/webm", b"audio").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &au.id, "서준이가 별은 왜 반짝여요? 하고 물었어요. 지우는 몰랐어요").unwrap();
    obs(&mut cx, "2027-04-11", "박서준과 짝이 되어 돋보기로 관찰함");
    obs(&mut cx, "2027-04-12", "교실을 살펴봄");

    let b = bundle_of(&cx, 0, "슬");
    let json = serde_json::to_string(&b).unwrap();
    for name in ["박서준", "서준", "이지우", "지우", "김하늘"] {
        assert!(!json.contains(name), "{name} 이(가) 근거 묶음에 남아 있음: {json}");
    }
    assert_eq!(b.student.alias, "학생");
    assert!(json.contains("친구"));
    assert!(json.contains("\"alias\":\"학생\""));
    // 함께 기록된 관찰은 표시만 한다.
    let together: Vec<bool> = b.evidence.iter().filter_map(|e| if let Evidence::Observation { together, .. } = e { Some(*together) } else { None }).collect();
    assert_eq!(together.iter().filter(|t| **t).count(), 1);
}

#[test]
fn bundle_json_has_the_documented_shape() {
    let mut cx = seed();
    let o = obs_full(&mut cx, &[0], "2027-04-10", "g1s1-exploration", "우주 영상을 보고 질문함", &["[2슬01-01]"], &["탐구", "질문"]);
    seed_inquiry(&mut cx, 2);
    let b = bundle_of(&cx, 0, "슬");
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["student"]["alias"], "학생");
    assert_eq!(v["subject"], "슬");
    assert_eq!(v["period"]["from"], FROM);
    let first = v["evidence"].as_array().unwrap().iter().find(|e| e["id"] == format!("o:{}", o.id)).unwrap();
    assert_eq!(first["kind"], "observation");
    assert_eq!(first["date"], "2027-04-10");
    assert_eq!(first["topic"], "탐험");
    assert_eq!(first["standards"][0], "[2슬01-01]");
    assert_eq!(first["together"], false);
    assert!(first.get("quote").is_none());
    assert!(v["standardsText"]["[2슬01-01]"].is_string());
}

#[test]
fn bundle_filters_period_deleted_other_students_empty_notes_and_other_subjects() {
    let mut cx = seed();
    obs(&mut cx, "2027-04-01", "포함되는 기록");
    obs(&mut cx, "2027-02-28", "기간 밖");
    obs(&mut cx, "2027-07-19", "기간 밖");
    let gone = obs(&mut cx, "2027-04-02", "삭제됨");
    observations::delete(&cx.conn, &gone.id).unwrap();
    obs_full(&mut cx, &[1], "2027-04-03", "g1s1-exploration", "서준 혼자", &["[2슬01-01]"], &["탐구"]);
    obs(&mut cx, "2027-04-04", "");
    // 바른 생활 성취기준만 붙은 기록은 슬기로운 생활 묶음 앞쪽이 아니라 나머지로 밀린다.
    obs_full(&mut cx, &[0], "2027-04-05", "g1s1-school", "인사를 나눔", &["[2바01-01]"], &["실천"]);
    let b = bundle_of(&cx, 0, "슬");
    let texts: Vec<&str> = b.evidence.iter().filter_map(|e| if let Evidence::Observation { text, .. } = e { Some(text.as_str()) } else { None }).collect();
    assert!(texts.contains(&"포함되는 기록"));
    for bad in ["기간 밖", "삭제됨", "서준 혼자", ""] {
        assert!(!texts.contains(&bad), "{bad}");
    }
    assert_eq!(texts.len(), 2, "{texts:?}");
}

#[test]
fn bundle_selection_orders_standards_then_tags_then_rest_and_caps_at_12_mixing_topics() {
    let mut cx = seed();
    // 성취기준 붙은 기록 8개(주제책 둘, 각 4개), 성격 태그만 붙은 기록 6개
    let mut group0 = vec![];
    for i in 0..8 {
        let topic = if i < 4 { "g1s1-exploration" } else { "g1s1-school" };
        group0.push(format!("o:{}", obs_full(&mut cx, &[0], &format!("2027-04-{:02}", i + 1), topic, &format!("기록{i}"), &["[2슬01-01]"], &["탐구"]).id));
    }
    for i in 0..6 {
        obs_full(&mut cx, &[0], &format!("2027-05-{:02}", i + 1), "g1s1-school", &format!("태그{i}"), &[], &["질문"]);
    }
    obs_full(&mut cx, &[0], "2027-06-01", "g1s1-school", "나머지", &[], &["놀이"]);
    let b = bundle_of(&cx, 0, "슬");
    assert_eq!(b.evidence.len(), 12);
    let ids: Vec<&str> = b.evidence.iter().map(Evidence::id).collect();
    for id in &group0 {
        assert!(ids.contains(&id.as_str()), "성취기준 붙은 기록은 모두 들어간다");
    }
    let texts: Vec<&str> = b.evidence.iter().filter_map(|e| if let Evidence::Observation { text, .. } = e { Some(text.as_str()) } else { None }).collect();
    assert_eq!(texts.iter().filter(|t| t.starts_with("태그")).count(), 4);
    assert!(!texts.contains(&"나머지"));
    // 날짜 오름차순으로 나열
    let dates: Vec<&str> = b.evidence.iter().filter_map(|e| if let Evidence::Observation { date, .. } = e { Some(date.as_str()) } else { None }).collect();
    assert!(dates.windows(2).all(|w| w[0] <= w[1]));

    // 주제책이 고르게 섞인다: 같은 묶음 안에서 한 주제책이 몰리지 않는다.
    let mut cx2 = seed();
    for i in 0..9 {
        obs_full(&mut cx2, &[0], &format!("2027-04-{:02}", i + 1), "g1s1-exploration", &format!("탐험{i}"), &["[2슬01-01]"], &["탐구"]);
    }
    for i in 0..9 {
        obs_full(&mut cx2, &[0], &format!("2027-05-{:02}", i + 1), "g1s1-school", &format!("학교{i}"), &["[2슬01-01]"], &["탐구"]);
    }
    let b2 = bundle_of(&cx2, 0, "슬");
    let school = b2.evidence.iter().filter(|e| matches!(e, Evidence::Observation { topic: Some(t), .. } if t == "학교")).count();
    assert_eq!(b2.evidence.len(), 12);
    assert_eq!(school, 6);
}

#[test]
fn bundle_quote_only_from_done_transcripts_and_limited_to_80_chars() {
    let mut cx = seed();
    let o1 = obs(&mut cx, "2027-04-01", "자막 있음");
    let o2 = obs(&mut cx, "2027-04-02", "자막 만드는 중");
    let o3 = obs(&mut cx, "2027-04-03", "");
    let long = "가".repeat(120);
    let a1 = attachments::add_bytes(&cx.conn, &cx.media, &o1.id, "audio", "audio/webm", b"x").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &a1.id, &long).unwrap();
    let a2 = attachments::add_bytes(&cx.conn, &cx.media, &o2.id, "audio", "audio/webm", b"x").unwrap();
    cx.conn.execute("UPDATE attachments SET transcript = '아직 확인 안 된 자막', transcript_status = 'running' WHERE id = ?1", [&a2.id]).unwrap();
    // 메모는 없지만 자막이 있는 기록은 자막을 근거로 쓴다.
    let a3 = attachments::add_bytes(&cx.conn, &cx.media, &o3.id, "audio", "audio/webm", b"x").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &a3.id, "개미가 줄을 서서 가요").unwrap();
    let b = bundle_of(&cx, 0, "슬");
    let quote_of = |oid: &str| b.evidence.iter().find_map(|e| match e {
        Evidence::Observation { id, quote, .. } if *id == format!("o:{oid}") => Some(quote.clone()),
        _ => None,
    });
    assert_eq!(quote_of(&o1.id).unwrap().unwrap().chars().count(), 80);
    assert_eq!(quote_of(&o2.id).unwrap(), None);
    assert_eq!(quote_of(&o3.id).unwrap().as_deref(), Some("개미가 줄을 서서 가요"));
    assert!(!serde_json::to_string(&b).unwrap().contains("아직 확인 안 된"));
}

#[test]
fn habits_only_for_bareun_and_sessions_follow_subject_standards() {
    let mut cx = seed();
    obs_full(&mut cx, &[0], "2027-04-01", "g1s1-school", "인사를 나눔", &["[2바01-01]"], &["실천"]);
    obs(&mut cx, "2027-04-02", "관찰함");
    cx.conn.execute_batch(&format!(
        "INSERT INTO habits (id, class_id, title, created_at) VALUES ('hb1', '{c}', '사물함 정리하기', 't');
         INSERT INTO habit_checks (habit_id, student_id, checked_on, done, updated_at) VALUES
           ('hb1', '{s0}', '2027-04-01', 1, 't'), ('hb1', '{s0}', '2027-04-02', 1, 't'), ('hb1', '{s0}', '2027-04-03', 0, 't'),
           ('hb1', '{s1}', '2027-04-01', 1, 't'), ('hb1', '{s1}', '2027-04-02', 1, 't'), ('hb1', '{s1}', '2027-04-03', 1, 't');
         INSERT INTO units (id, class_id, topic_book_id, tool_id, title, created_at, updated_at) VALUES ('u1', '{c}', 'g1s1-exploration', 'free', '탐험 단원', 't', 't');
         INSERT INTO unit_stages (id, unit_id, position, title) VALUES ('st1', 'u1', 0, '단계');
         INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, status, scheduled_on, created_at, updated_at)
           VALUES ('se1', 'u1', 'st1', 0, '우주 영상 보기', 'done', '2027-04-05', 't', 't'),
                  ('se2', 'u1', 'st1', 1, '계획만 한 차시', 'planned', '2027-04-06', 't', 't'),
                  ('se3', 'u1', 'st1', 2, '다른 교과 차시', 'done', '2027-04-07', 't', 't');
         INSERT INTO unit_session_standards (session_id, standard_code, mode) VALUES
           ('se1', '[2슬01-01]', 'focus'), ('se2', '[2슬01-01]', 'focus'), ('se3', '[2바01-01]', 'focus');",
        c = cx.class, s0 = cx.ids[0], s1 = cx.ids[1]
    ))
    .unwrap();
    let ba = bundle_of(&cx, 0, "바");
    let habit = ba.evidence.iter().find_map(|e| if let Evidence::Habit { title, done_days, school_days, .. } = e { Some((title.clone(), *done_days, *school_days)) } else { None });
    assert_eq!(habit, Some(("사물함 정리하기".to_string(), 2, 3)), "이 아이의 실천 횟수");
    let seul = bundle_of(&cx, 0, "슬");
    assert!(!seul.evidence.iter().any(|e| matches!(e, Evidence::Habit { .. })));
    let sessions: Vec<&str> = seul.evidence.iter().filter_map(|e| if let Evidence::Session { title, .. } = e { Some(title.as_str()) } else { None }).collect();
    assert_eq!(sessions, ["우주 영상 보기"], "완료된, 그 교과 성취기준이 붙은 차시만");
    let ba_sessions: Vec<&str> = ba.evidence.iter().filter_map(|e| if let Evidence::Session { title, .. } = e { Some(title.as_str()) } else { None }).collect();
    assert_eq!(ba_sessions, ["다른 교과 차시"]);
    // 이 아이가 아닌 학생의 습관 횟수는 들어가지 않는다.
    let json = serde_json::to_string(&ba).unwrap();
    assert!(!json.contains("\"doneDays\":3"));
}

#[test]
fn pinned_evidence_outside_the_cap_stays_in_the_bundle() {
    let mut cx = seed();
    let ids = seed_inquiry(&mut cx, 14);
    let b = bundle_of(&cx, 0, "슬");
    assert_eq!(b.evidence.len(), 12);
    let missing = ids.iter().map(|i| format!("o:{i}")).find(|i| !b.evidence.iter().any(|e| e.id() == i)).unwrap();
    let pinned = bundle::build(&cx.conn, &cx.media, &cx.ids[0], "슬", FROM, TO, std::slice::from_ref(&missing)).unwrap();
    assert_eq!(pinned.evidence.len(), 13);
    assert!(pinned.evidence.iter().any(|e| e.id() == missing));
    // 없는 id를 고정해도 늘어나지 않는다.
    let none = bundle::build(&cx.conn, &cx.media, &cx.ids[0], "슬", FROM, TO, &["o:nope".to_string()]).unwrap();
    assert_eq!(none.evidence.len(), 12);
}

#[test]
fn bundle_rejects_bad_subject_and_period() {
    let cx = seed();
    assert!(bundle::build(&cx.conn, &cx.media, &cx.ids[0], "수", FROM, TO, &[]).is_err());
    assert!(bundle::build(&cx.conn, &cx.media, &cx.ids[0], "슬", TO, FROM, &[]).is_err());
    assert!(bundle::build(&cx.conn, &cx.media, "nope", "슬", FROM, TO, &[]).is_err());
}

// ---------- (2) 생성 ----------

#[test]
fn fewer_than_three_evidence_refuses_to_generate_and_saves_nothing() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 2);
    let input = GenerateInput { key: key(&cx, 0, "슬"), excluded_ids: vec![] };
    let err = generate_draft(&cx.conn, &cx.media, &input).unwrap_err();
    assert_eq!(err, "기록이 부족합니다(2개). 이 교과 관찰 기록을 더 남겨 주세요");
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    let v = view(&cx.conn, &cx.media, &input.key).unwrap();
    assert!(!v.enough && v.draft.is_none());

    // 체크를 풀어 3개 미만이 되어도 만들지 않는다.
    seed_inquiry(&mut cx, 2);
    let all = bundle_of(&cx, 0, "슬");
    assert!(all.evidence.len() >= 4);
    let excluded: Vec<String> = all.evidence.iter().skip(2).map(|e| e.id().to_string()).collect();
    let err = generate_draft(&cx.conn, &cx.media, &GenerateInput { key: key(&cx, 0, "슬"), excluded_ids: excluded }).unwrap_err();
    assert!(err.contains("기록이 부족합니다(2개)"), "{err}");
}

#[test]
fn template_output_is_deterministic_and_every_sentence_cites_evidence() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 6);
    for subject in ["바", "슬", "즐"] {
        // 모든 교과에서 같은 기록이 쓰이도록 성취기준 없이 태그로도 걸리게 한다.
        let b = bundle_of(&cx, 0, subject);
        for style in [Style::Ham, Style::Eum, Style::Boim] {
            let a = generate(&b, style, 250, "seed-1", forbidden(), &[]);
            let again = generate(&b.clone(), style, 250, "seed-1", forbidden(), &[]);
            assert_eq!(a, again, "{subject} {style:?}");
            if b.evidence.len() >= 3 {
                assert!((2..=4).contains(&a.len()), "{subject}: {}개", a.len());
            }
            for s in &a {
                assert!(!s.evidence.is_empty());
                assert!(s.evidence.iter().all(|id| b.evidence.iter().any(|e| e.id() == id)), "근거는 묶음 안의 id여야 한다");
                assert!(!s.text.contains('{') && !s.text.contains('}'), "{}", s.text);
            }
        }
    }
    // 저장된 초안도 다시 만들면 같다.
    let input = GenerateInput { key: key(&cx, 0, "슬"), excluded_ids: vec![] };
    let first = generate_draft(&cx.conn, &cx.media, &input).unwrap().draft.unwrap().sentences;
    let second = generate_draft(&cx.conn, &cx.media, &input).unwrap().draft.unwrap().sentences;
    assert_eq!(first, second);
}

#[test]
fn style_setting_changes_the_sentence_ending() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 5);
    let input = GenerateInput { key: key(&cx, 0, "슬"), excluded_ids: vec![] };
    let expect = [("ham", "함."), ("eum", "하였음."), ("boim", "하는 모습을 보임.")];
    for (style, ending) in expect {
        let s = EvalSettings { style: style.to_string(), target_chars: 250, extra_forbidden: vec![] };
        save_settings(&cx.conn, &s).unwrap();
        let d = generate_draft(&cx.conn, &cx.media, &input).unwrap().draft.unwrap();
        assert_eq!(d.generator, "template");
        for s in &d.sentences {
            assert!(s.text.ends_with(ending), "{style}: {}", s.text);
        }
    }
}

#[test]
fn different_students_get_varied_phrasings() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 6);
    let b = bundle_of(&cx, 0, "슬");
    let outputs: std::collections::HashSet<Vec<String>> = (0..12)
        .map(|i| generate(&b, Style::Ham, 250, &format!("student-{i}"), forbidden(), &[]).into_iter().map(|s| s.text).collect())
        .collect();
    assert!(outputs.len() >= 6, "시드마다 문장이 달라야 한다: {}가지", outputs.len());
}

#[test]
fn note_memo_and_quote_are_woven_into_sentences() {
    let mut cx = seed();
    let o = obs_full(&mut cx, &[0], "2027-04-01", "g1s1-exploration", "친구를 도와줌", &["[2슬01-01]"], &["질문"]);
    let a = attachments::add_bytes(&cx.conn, &cx.media, &o.id, "audio", "audio/webm", b"x").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &a.id, "별은 왜 반짝여요?").unwrap();
    seed_inquiry(&mut cx, 3);
    let b = bundle_of(&cx, 0, "슬");
    // 시드를 바꿔 가며 보면 메모·자막을 쓴 문장이 적어도 한 번은 나온다.
    let mut saw_note = false;
    let mut saw_quote = false;
    for i in 0..40 {
        for s in generate(&b, Style::Ham, 400, &format!("s{i}"), forbidden(), &[]) {
            saw_note |= s.text.contains("친구를 도와주는 ");
            saw_quote |= s.text.contains("'별은 왜 반짝여요?'라고");
        }
    }
    assert!(saw_note && saw_quote, "note={saw_note} quote={saw_quote}");
}

#[test]
fn generator_skips_memo_text_that_hits_forbidden_words() {
    let mut cx = seed();
    obs_full(&mut cx, &[0], "2027-04-01", "g1s1-exploration", "반에서 가장 빨리 찾아봄", &["[2슬01-01]"], &["탐구"]);
    seed_inquiry(&mut cx, 3);
    let b = bundle_of(&cx, 0, "슬");
    for i in 0..30 {
        for s in generate(&b, Style::Ham, 400, &format!("s{i}"), forbidden(), &[]) {
            assert!(forbidden().find(&s.text, &[]).is_empty(), "{}", s.text);
        }
    }
}

#[test]
fn generated_drafts_pass_every_automatic_check() {
    let mut cx = seed();
    let ids = seed_inquiry(&mut cx, 8);
    let o = observations::get(&cx.conn, &cx.media, &ids[0]).unwrap();
    let a = attachments::add_bytes(&cx.conn, &cx.media, &o.id, "audio", "audio/webm", b"x").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &a.id, "개미는 어디로 가요?").unwrap();
    let mut checked = 0;
    for subject in ["바", "슬", "즐"] {
        for style in ["ham", "eum", "boim"] {
            save_settings(&cx.conn, &EvalSettings { style: style.to_string(), target_chars: 250, extra_forbidden: vec![] }).unwrap();
            let v = generate_draft(&cx.conn, &cx.media, &GenerateInput { key: key(&cx, 0, subject), excluded_ids: vec![] }).unwrap();
            checked += 1;
            let c = v.check.unwrap();
            for (i, w) in c.sentences.iter().enumerate() {
                assert!(w.is_empty(), "{subject} {style} {}번째 문장 경고: {w:?} / {}", i + 1, v.draft.as_ref().unwrap().sentences[i].text);
            }
            assert!(c.can_confirm);
        }
    }
    assert_eq!(checked, 9);
}

#[test]
fn every_template_group_has_variants_and_never_trips_forbidden_words() {
    let t = templates();
    let mut groups: std::collections::HashMap<(String, String, String), (usize, usize)> = Default::default();
    for x in &t.templates {
        let free = !x.text.replace("{E}", "").contains('{');
        let e = groups.entry((x.kind.clone(), x.subject.clone(), x.group.clone())).or_default();
        e.0 += 1;
        e.1 += usize::from(free);
        for ending in [&t.endings.ham, &t.endings.eum, &t.endings.boim] {
            let text = x.text.replace("{E}", ending);
            assert!(forbidden().find(&text, &[]).is_empty(), "{}: {text}", x.id);
        }
    }
    for subject in ["바", "슬", "즐"] {
        let mut want: Vec<String> = t.tag_groups[subject].values().cloned().collect();
        want.push("general".to_string());
        for g in want {
            let (n, _) = groups.get(&("observation".to_string(), subject.to_string(), g.clone())).copied().unwrap_or_default();
            assert!(n >= 4, "{subject}/{g}: {n}개");
        }
    }
    for ((kind, subject, group), (n, _)) in &groups {
        assert!(*n >= 3, "{kind}/{subject}/{group}");
    }
}

// ---------- (3) 자동 검사 ----------

fn check_with(cx: &Cx, sentences: &[Sentence]) -> check::DraftCheck {
    let b = bundle_of(cx, 0, "슬");
    let others = vec!["박서준".to_string(), "서준".to_string(), "이지우".to_string(), "지우".to_string()];
    check_draft(sentences, &b, forbidden(), &[], &others)
}

#[test]
fn check_flags_sentences_without_evidence_or_with_unknown_ids() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    let b = bundle_of(&cx, 0, "슬");
    let good = b.evidence[0].id().to_string();
    let c = check_with(
        &cx,
        &[sentence("돋보기로 관찰하며 탐구함.", &[]), sentence("돋보기로 관찰하며 탐구함.", &["o:없는-id"]), sentence("돋보기로 관찰하며 탐구함.", &[&good])],
    );
    assert_eq!(codes_of(&c.sentences[0]), [NO_EVIDENCE]);
    assert_eq!(codes_of(&c.sentences[1]), [NO_EVIDENCE]);
    assert!(!codes_of(&c.sentences[2]).contains(&NO_EVIDENCE));
    assert!(!c.can_confirm);
    // 빈 목록과 빈 문장도 확정 불가
    assert!(!check_with(&cx, &[]).can_confirm);
    assert!(!check_with(&cx, &[sentence("  ", &[&good])]).can_confirm);
}

#[test]
fn check_flags_forbidden_phrases_including_teacher_extras() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    let b = bundle_of(&cx, 0, "슬");
    let id = b.evidence[0].id().to_string();
    let others: Vec<String> = vec![];
    let run = |text: &str, extra: &[String]| check_draft(&[sentence(text, &[&id])], &b, forbidden(), extra, &others).sentences.remove(0);
    let w = run("반에서 가장 빠르게 관찰함.", &[]);
    assert!(w.iter().any(|x| x.code == FORBIDDEN && x.message.contains("가장") && x.message.contains("서열")), "{w:?}");
    assert!(run("학원에서 배운 대로 관찰함.", &[]).iter().any(|x| x.code == FORBIDDEN && x.message.contains("사교육")));
    assert!(run("관찰하며 탐구함.", &[]).iter().all(|x| x.code != FORBIDDEN));
    let extra = vec!["해바라기반".to_string()];
    assert!(run("해바라기반 활동에서 관찰함.", &extra).iter().any(|x| x.code == FORBIDDEN && x.message.contains("해바라기반")));
    assert!(run("해바라기반 활동에서 관찰함.", &[]).iter().all(|x| x.code != FORBIDDEN));
}

#[test]
fn check_flags_other_students_names() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    let b = bundle_of(&cx, 0, "슬");
    let id = b.evidence[0].id().to_string();
    let c = check_with(&cx, &[sentence("서준이와 함께 돋보기로 관찰함.", &[&id]), sentence("친구와 함께 돋보기로 관찰함.", &[&id]), sentence("이지우에게 설명하며 탐구함.", &[&id])]);
    assert!(codes_of(&c.sentences[0]).contains(&OTHER_NAME));
    assert!(!codes_of(&c.sentences[1]).contains(&OTHER_NAME));
    assert!(codes_of(&c.sentences[2]).contains(&OTHER_NAME));
}

#[test]
fn check_warns_when_sentence_has_no_word_in_common_with_the_cited_evidence() {
    let mut cx = seed();
    let o = obs_full(&mut cx, &[0], "2027-04-01", "g1s1-exploration", "돋보기로 곤충을 자세히 관찰함", &["[2슬01-01]"], &["탐구"]);
    seed_inquiry(&mut cx, 3);
    let id = format!("o:{}", o.id);
    let c = check_with(
        &cx,
        &[
            sentence("공룡 박물관에서 화석을 발굴하며 탐구함.", &[&id]),
            sentence("돋보기로 곤충을 관찰하며 탐구함.", &[&id]),
            sentence("탐험 활동에서 알게 된 점을 표현함.", &[&id]),
            sentence("공룡 박물관에서 화석을 발굴하며 탐구함.", &[&format!("o:{}", o.id), "o:없음"]),
        ],
    );
    assert!(codes_of(&c.sentences[0]).contains(&MISMATCH), "{:?}", c.sentences[0]);
    assert!(!codes_of(&c.sentences[1]).contains(&MISMATCH));
    assert!(!codes_of(&c.sentences[2]).contains(&MISMATCH), "주제책 이름은 근거에 있다");
    // 일부만 겹쳐도 경고하지 않는다(겹침 0일 때만).
    let c2 = check_with(&cx, &[sentence("돋보기로 공룡 화석을 관찰함.", &[&id])]);
    assert!(!codes_of(&c2.sentences[0]).contains(&MISMATCH));
}

#[test]
fn check_warns_about_length_outside_80_to_400() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    let b = bundle_of(&cx, 0, "슬");
    let id = b.evidence[0].id().to_string();
    let run = |n: usize| check_draft(&[sentence(&"가".repeat(n), &[&id])], &b, forbidden(), &[], &[]);
    assert!(run(50).length.unwrap().message.contains("짧"));
    assert!(run(79).length.unwrap().message.contains("짧"));
    assert!(run(80).length.is_none());
    assert!(run(150).length.is_none());
    assert!(run(400).length.is_none());
    let long = run(401).length.unwrap();
    assert_eq!(long.code, LENGTH);
    assert!(long.message.contains("깁니다"));
}

// ---------- (4) 저장·확정·내보내기 ----------

#[test]
fn confirm_is_blocked_while_any_sentence_lacks_evidence() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    let b = bundle_of(&cx, 0, "슬");
    let id = b.evidence[0].id().to_string();
    let k = key(&cx, 0, "슬");

    assert!(confirm(&cx.conn, &cx.media, &k).unwrap_err().contains("초안이 없습니다"));

    // 근거 없는 문장 하나가 섞인 초안
    let save = |sentences: Vec<Sentence>| save_draft(&cx.conn, &cx.media, &SaveInput { key: k.clone(), sentences }).unwrap();
    save(vec![sentence("돋보기로 관찰하며 탐구함.", &[&id]), sentence("직접 추가한 문장함.", &[])]);
    let err = confirm(&cx.conn, &cx.media, &k).unwrap_err();
    assert!(err.contains("2번째 문장에 근거가 없습니다"), "{err}");
    // 묶음에 없는 id도 근거로 치지 않는다.
    save(vec![sentence("돋보기로 관찰하며 탐구함.", &[&id]), sentence("직접 추가한 문장함.", &["o:없음"])]);
    assert!(confirm(&cx.conn, &cx.media, &k).is_err());
    let v = view(&cx.conn, &cx.media, &k).unwrap();
    assert_eq!(v.draft.as_ref().unwrap().status, "draft");
    assert!(!v.check.unwrap().can_confirm);

    // 근거를 붙이면 확정된다.
    save(vec![sentence("돋보기로 관찰하며 탐구함.", &[&id]), sentence("직접 추가한 문장함.", &[&id])]);
    let v = confirm(&cx.conn, &cx.media, &k).unwrap();
    let d = v.draft.unwrap();
    assert_eq!(d.status, "confirmed");
    assert!(d.confirmed_at.is_some());

    // 고치면 다시 초안이 되고 확정 시각이 지워진다.
    let v = save(vec![sentence("돋보기로 관찰하며 탐구함.", &[&id])]);
    let d = v.draft.unwrap();
    assert_eq!((d.status.as_str(), d.confirmed_at), ("draft", None));
}

#[test]
fn saving_validates_sentences_and_keeps_generator() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 5);
    let k = key(&cx, 0, "슬");
    let id = bundle_of(&cx, 0, "슬").evidence[0].id().to_string();
    let save = |sentences: Vec<Sentence>| save_draft(&cx.conn, &cx.media, &SaveInput { key: k.clone(), sentences });
    assert!(save(vec![sentence("  ", &[&id])]).unwrap_err().contains("비어 있습니다"));
    assert!(save((0..13).map(|_| sentence("문장", &[&id])).collect()).is_err());
    assert!(save(vec![sentence(&"가".repeat(501), &[&id])]).is_err());
    let v = save(vec![sentence("  돋보기로   관찰함. ", &[&id, &id, " "])]).unwrap();
    let d = v.draft.unwrap();
    assert_eq!(d.generator, "manual");
    assert_eq!(d.sentences, [sentence("돋보기로 관찰함.", &[&id])], "공백 정리, 근거 중복 제거");
    // 템플릿 초안을 고쳐 저장해도 생성기 종류는 그대로 남는다.
    generate_draft(&cx.conn, &cx.media, &GenerateInput { key: k.clone(), excluded_ids: vec![] }).unwrap();
    let d = save(vec![sentence("고친 문장함.", &[&id])]).unwrap().draft.unwrap();
    assert_eq!(d.generator, "template");
    // 한 학생·교과·기간에 초안은 하나.
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn view_flags_edited_sentences_with_other_names_and_forbidden_words() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    let k = key(&cx, 0, "슬");
    let id = bundle_of(&cx, 0, "슬").evidence[0].id().to_string();
    save_settings(&cx.conn, &EvalSettings { style: "ham".into(), target_chars: 250, extra_forbidden: vec!["해바라기반".into()] }).unwrap();
    let v = save_draft(
        &cx.conn,
        &cx.media,
        &SaveInput { key: k.clone(), sentences: vec![sentence("박서준과 돋보기로 관찰함.", &[&id]), sentence("가장 열심히 해바라기반 활동을 함.", &[&id])] },
    )
    .unwrap();
    let c = v.check.unwrap();
    assert!(codes_of(&c.sentences[0]).contains(&OTHER_NAME));
    let f = codes_of(&c.sentences[1]).iter().filter(|c| **c == FORBIDDEN).count();
    assert_eq!(f, 2, "가장 + 교사가 더한 해바라기반");
    // 편집 중 검사 명령도 같은 결과
    let live = check_sentences(&cx.conn, &cx.media, &SaveInput { key: k, sentences: vec![sentence("박서준과 돋보기로 관찰함.", &[&id])] }).unwrap();
    assert!(codes_of(&live.sentences[0]).contains(&OTHER_NAME));
}

#[test]
fn class_status_lists_counts_and_progress() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    obs_full(&mut cx, &[1], "2027-04-01", "g1s1-exploration", "서준 기록", &["[2슬01-01]"], &["탐구"]);
    generate_draft(&cx.conn, &cx.media, &GenerateInput { key: key(&cx, 0, "슬"), excluded_ids: vec![] }).unwrap();
    let rows = class_status(&cx.conn, &cx.media, &cx.class, "슬", FROM, TO).unwrap();
    let got: Vec<(i64, usize, &str)> = rows.iter().map(|r| (r.number, r.evidence_count, r.status.as_str())).collect();
    assert_eq!(got, [(1, 4, "draft"), (2, 1, "none"), (3, 0, "none")]);
    confirm(&cx.conn, &cx.media, &key(&cx, 0, "슬")).unwrap();
    assert_eq!(class_status(&cx.conn, &cx.media, &cx.class, "슬", FROM, TO).unwrap()[0].status, "confirmed");
    // 다른 교과·기간의 초안은 섞이지 않는다.
    assert_eq!(class_status(&cx.conn, &cx.media, &cx.class, "즐", FROM, TO).unwrap()[0].status, "none");
    assert_eq!(class_status(&cx.conn, &cx.media, &cx.class, "슬", "2027-08-01", "2027-12-31").unwrap()[0].status, "none");
}

#[test]
fn export_contains_only_confirmed_drafts_and_is_written_with_a_bom() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    obs_full(&mut cx, &[1], "2027-04-01", "g1s1-exploration", "서준 기록", &["[2슬01-01]"], &["탐구"]);
    assert!(export_text(&cx.conn, &cx.class, FROM, TO).unwrap_err().contains("확정된 평어가 없습니다"));
    let id = bundle_of(&cx, 0, "슬").evidence[0].id().to_string();
    // 하늘: 확정, 서준: 초안만
    save_draft(&cx.conn, &cx.media, &SaveInput { key: key(&cx, 0, "슬"), sentences: vec![sentence("돋보기로\t관찰하며\n탐구함.", &[&id]), sentence("끝까지 살펴봄.", &[&id])] }).unwrap();
    confirm(&cx.conn, &cx.media, &key(&cx, 0, "슬")).unwrap();
    let id1 = bundle_of(&cx, 1, "슬").evidence[0].id().to_string();
    save_draft(&cx.conn, &cx.media, &SaveInput { key: key(&cx, 1, "슬"), sentences: vec![sentence("서준 문장함.", &[&id1])] }).unwrap();

    let (text, n) = export_text(&cx.conn, &cx.class, FROM, TO).unwrap();
    assert_eq!(n, 1);
    assert_eq!(text, "번호\t이름\t교과\t평어\r\n1\t김하늘\t슬기로운 생활\t돋보기로 관찰하며 탐구함. 끝까지 살펴봄.\r\n");
    assert!(!text.contains("서준 문장"));

    let dir = tmp("export");
    let path = dir.join("평어.txt");
    write_text_file(path.to_str().unwrap(), &text).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], b"\xEF\xBB\xBF");
    assert_eq!(std::str::from_utf8(&bytes[3..]).unwrap(), text);
    // 덮어쓰기와 임시 파일 정리
    write_text_file(path.to_str().unwrap(), "다시 씀").unwrap();
    assert_eq!(std::fs::read(&path).unwrap().len(), 3 + "다시 씀".len());
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, ["평어.txt"]);
}

#[test]
fn write_text_file_refuses_unsafe_targets() {
    let dir = tmp("unsafe");
    let ok = dir.join("a.txt");
    assert!(write_text_file(dir.join("a.png").to_str().unwrap(), "x").unwrap_err().contains(".txt"));
    assert!(write_text_file(dir.join("a.exe").to_str().unwrap(), "x").is_err());
    assert!(write_text_file("상대경로.txt", "x").unwrap_err().contains("경로"));
    assert!(write_text_file(dir.join("없는폴더").join("a.txt").to_str().unwrap(), "x").is_err());
    assert!(write_text_file(ok.to_str().unwrap(), "  ").is_err());
    std::fs::create_dir(dir.join("폴더.txt")).unwrap();
    assert!(write_text_file(dir.join("폴더.txt").to_str().unwrap(), "x").unwrap_err().contains("폴더"));
    assert!(!ok.exists());
}

#[test]
fn settings_default_roundtrip_and_validation() {
    let cx = seed();
    let d = load_settings(&cx.conn).unwrap();
    assert_eq!(d, EvalSettings { style: "ham".to_string(), target_chars: DEFAULT_TARGET, extra_forbidden: vec![] });
    assert_eq!(DEFAULT_TARGET, 150);
    let saved = save_settings(&cx.conn, &EvalSettings { style: "boim".into(), target_chars: 300, extra_forbidden: vec![" 해바라기반 ".into(), "".into(), "해바라기반".into(), "별빛교실".into()] }).unwrap();
    assert_eq!(saved.style, "boim");
    assert_eq!(saved.target_chars, 300);
    assert_eq!(saved.extra_forbidden, ["해바라기반", "별빛교실"]);
    assert!(save_settings(&cx.conn, &EvalSettings { style: "xx".into(), target_chars: 250, extra_forbidden: vec![] }).is_err());
    assert!(save_settings(&cx.conn, &EvalSettings { style: "ham".into(), target_chars: 79, extra_forbidden: vec![] }).is_err());
    assert!(save_settings(&cx.conn, &EvalSettings { style: "ham".into(), target_chars: 80, extra_forbidden: vec![] }).is_ok());
    assert!(save_settings(&cx.conn, &EvalSettings { style: "ham".into(), target_chars: 401, extra_forbidden: vec![] }).is_err());
    // 잘못 저장된 값은 기본값으로 읽는다.
    settings::set(&cx.conn, KEY_STYLE, &serde_json::json!("nope")).unwrap();
    settings::set(&cx.conn, KEY_TARGET, &serde_json::json!(9999)).unwrap();
    let r = load_settings(&cx.conn).unwrap();
    assert_eq!((r.style.as_str(), r.target_chars), ("ham", 150));
}

#[test]
fn deleting_a_student_removes_their_drafts() {
    let mut cx = seed();
    seed_inquiry(&mut cx, 4);
    generate_draft(&cx.conn, &cx.media, &GenerateInput { key: key(&cx, 0, "슬"), excluded_ids: vec![] }).unwrap();
    cx.conn.execute("DELETE FROM students WHERE id = ?1", [&cx.ids[0]]).unwrap();
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

// ---------- 근거 내용을 담는 문장 ----------

#[test]
fn every_observation_template_carries_note_or_quote_and_no_template_says_moseup() {
    for t in &templates().templates {
        assert!(!t.text.contains("모습"), "{}: 본문에 '모습' 금지", t.id);
        if t.kind == "observation" {
            assert!(t.text.contains("{note}") || t.text.contains("{quote}"), "{}: {{topic}}만으로는 근거를 담지 못한다", t.id);
        }
    }
    // 자막만 있는 기록을 위해 묶음마다 {quote} 틀이, 메모용 {note} 틀이 2개 이상 있다.
    let mut groups: std::collections::HashMap<(String, String), (usize, usize)> = Default::default();
    for t in templates().templates.iter().filter(|t| t.kind == "observation") {
        let e = groups.entry((t.subject.clone(), t.group.clone())).or_default();
        e.0 += usize::from(t.text.contains("{note}"));
        e.1 += usize::from(t.text.contains("{quote}"));
    }
    assert_eq!(groups.len(), 11);
    for (g, (note, quote)) in groups {
        assert!(note >= 2 && quote >= 1, "{g:?}: note {note}, quote {quote}");
    }
}

/// 모든 교과 묶음을 거치도록 태그가 다른 관찰을 만든다.
fn seed_all_groups(cx: &mut Cx) -> Vec<String> {
    let memos = ["친구를 도와줌", "블록으로 탑을 쌓았음", "돋보기로 곤충을 관찰함", "규칙을 지키며 달렸습니다", "그림을 크게 그렸어요", "끝까지 찾아봄"];
    let tags = ["실천", "배려", "안전", "탐구", "질문", "놀이", "표현", "협력", "습관"];
    let mut ids = vec![];
    for (i, tag) in tags.iter().enumerate() {
        let o = obs_full(cx, &[0], &format!("2027-04-{:02}", i + 1), if i % 2 == 0 { "g1s1-school" } else { "g1s1-exploration" }, memos[i % memos.len()], &["[2슬01-01]", "[2바01-01]", "[2즐01-01]"], &[tag]);
        ids.push(o.id);
    }
    // 자막만 있는 기록(메모 없음)
    let q = obs_full(cx, &[0], "2027-05-01", "g1s1-school", "", &["[2슬01-01]", "[2바01-01]", "[2즐01-01]"], &["질문"]);
    let a = attachments::add_bytes(&cx.conn, &cx.media, &q.id, "audio", "audio/webm", b"x").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &a.id, "개미는 어디로 가요?").unwrap();
    ids.push(q.id);
    ids
}

fn content_words(text: &str) -> Vec<String> {
    check::tokens(text).into_iter().filter(|t| t.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c))).collect()
}

#[test]
fn every_generated_observation_sentence_contains_a_content_word_of_its_note_or_quote() {
    let mut cx = seed();
    seed_all_groups(&mut cx);
    let mut checked = 0;
    for subject in ["바", "슬", "즐"] {
        let b = bundle_of(&cx, 0, subject);
        for style in [Style::Ham, Style::Eum, Style::Boim] {
            for i in 0..30 {
                for s in generate(&b, style, 400, &format!("seed-{i}"), forbidden(), &[]) {
                    let ev = b.evidence.iter().find(|e| e.id() == s.evidence[0]).unwrap();
                    let Evidence::Observation { text, quote, .. } = ev else { continue };
                    let words: Vec<String> = content_words(text).into_iter().chain(content_words(quote.as_deref().unwrap_or(""))).collect();
                    let hit = words.iter().any(|w| check::variants(w).iter().filter(|v| v.chars().count() >= 2).any(|v| s.text.contains(v.as_str())));
                    assert!(hit, "[{subject} {style:?}] 근거 낱말이 문장에 없음: {} / 근거: {text:?} {quote:?}", s.text);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 300, "{checked}");
}

#[test]
fn quote_only_observation_gets_a_quote_sentence() {
    let mut cx = seed();
    let q = obs_full(&mut cx, &[0], "2027-04-01", "g1s1-exploration", "", &["[2슬01-01]"], &["질문"]);
    let a = attachments::add_bytes(&cx.conn, &cx.media, &q.id, "audio", "audio/webm", b"x").unwrap();
    attachments::set_transcript(&cx.conn, &cx.media, &a.id, "별은 왜 반짝여요?").unwrap();
    // 메모를 풀 수 없는 기록과 메모가 있는 기록
    obs_full(&mut cx, &[0], "2027-04-02", "g1s1-school", "알 수 없는 말투로 쓴 메모입니다만", &["[2슬01-01]"], &["탐구"]);
    obs(&mut cx, "2027-04-03", "돋보기로 관찰함");
    let b = bundle_of(&cx, 0, "슬");
    for style in [Style::Ham, Style::Eum, Style::Boim] {
        for i in 0..30 {
            let out = generate(&b, style, 400, &format!("q{i}"), forbidden(), &[]);
            let qid = format!("o:{}", q.id);
            for s in out.iter().filter(|s| s.evidence == [qid.clone()]) {
                assert!(s.text.contains("'별은 왜 반짝여요?'라고"), "{}", s.text);
            }
        }
    }
}

#[test]
fn boim_sentences_never_say_moseup_twice() {
    let mut cx = seed();
    seed_all_groups(&mut cx);
    cx.conn.execute_batch(&format!(
        "INSERT INTO habits (id, class_id, title, created_at) VALUES ('hb1', '{c}', '사물함 정리하기', 't');
         INSERT INTO habit_checks (habit_id, student_id, checked_on, done, updated_at) VALUES ('hb1', '{s}', '2027-04-01', 1, 't');",
        c = cx.class, s = cx.ids[0]
    ))
    .unwrap();
    let mut n = 0;
    for subject in ["바", "슬", "즐"] {
        let b = bundle_of(&cx, 0, subject);
        for i in 0..60 {
            for s in generate(&b, Style::Boim, 400, &format!("b{i}"), forbidden(), &[]) {
                assert!(s.text.matches("모습").count() <= 1, "{}", s.text);
                assert!(s.text.ends_with("하는 모습을 보임."));
                n += 1;
            }
        }
    }
    assert!(n > 300);
}

// ---------- (2') 언어 모델 생성기 ----------

mod llm_tests {
    use std::cell::RefCell;
    use std::sync::atomic::AtomicBool;

    use serde_json::{json, Value};

    use super::*;
    use crate::db::evaluation::llm::{self, build_request, parse_output, prompt, response_schema, seed_for, system_prompt, FALLBACK_NOTICE};
    use crate::llm_server::{LlmError, CANCELLED_MESSAGE};

    const MODEL: &str = "qwen3-4b-q4km";

    fn input(cx: &Cx, i: usize, subject: &str) -> GenerateInput {
        GenerateInput { key: key(cx, i, subject), excluded_ids: vec![] }
    }

    fn good_output(cx: &Cx) -> String {
        let b = bundle_of(cx, 0, "슬");
        let ids: Vec<&str> = b.evidence.iter().map(|e| e.id()).collect();
        json!({ "sentences": [
            { "text": "우주 영상을 보고 궁금한 점을 질문하며 주변 세계를 탐색함.", "evidence": [ids[0]] },
            { "text": "돋보기로 곤충을 자세히 관찰하고 발견한 점을 이야기함.", "evidence": [ids[1]] },
        ] })
        .to_string()
    }

    fn run_with(cx: &Cx, replies: Vec<Result<String, LlmError>>) -> (DbResult<llm::LlmDraftResult>, Vec<Value>) {
        let seen = RefCell::new(Vec::<Value>::new());
        let queue = RefCell::new(replies.into_iter());
        let chat = |req: &Value| -> Result<String, LlmError> {
            seen.borrow_mut().push(req.clone());
            queue.borrow_mut().next().unwrap_or_else(|| Err(LlmError::Failed("no more".into())))
        };
        let cancel = AtomicBool::new(false);
        let out = llm::generate_llm_draft(&cx.conn, &cx.media, &input(cx, 0, "슬"), MODEL, &chat, &cancel);
        (out, seen.into_inner())
    }

    fn cx_with_inquiry(n: usize) -> Cx {
        let mut cx = seed();
        seed_inquiry(&mut cx, n);
        cx
    }

    #[test]
    fn prompt_json_examples_pass_the_automatic_checks_in_every_style() {
        let p = prompt();
        assert!((2..=3).contains(&p.examples.len()));
        for ex in &p.examples {
            let ids: Vec<&str> = ex.bundle.evidence.iter().map(|e| e.id()).collect();
            assert!(ex.bundle.evidence.len() >= MIN_EVIDENCE);
            for (code, text) in &ex.bundle.standards_text {
                assert_eq!(super::super::data::standard_statement(code).as_deref(), Some(text.as_str()), "{} {code}", ex.id);
            }
            for style in ["ham", "eum", "boim"] {
                let sentences = &ex.outputs[style];
                let ending = match style {
                    "ham" => "함.",
                    "eum" => "하였음.",
                    _ => "하는 모습을 보임.",
                };
                assert!((2..=4).contains(&sentences.len()));
                for s in sentences {
                    assert!(s.text.ends_with(ending), "{}: {}", ex.id, s.text);
                    assert!(s.evidence.iter().all(|e| ids.contains(&e.as_str())));
                }
                let c = check_draft(sentences, &ex.bundle, forbidden(), &[], &[]);
                assert!(c.sentences.iter().all(|w| w.is_empty()), "{} {style}: {:?}", ex.id, c.sentences);
                assert!(c.length.is_none(), "{} {style}: {}자", ex.id, c.total_chars);
                assert!(c.can_confirm);
            }
        }
    }

    #[test]
    fn request_schema_enum_matches_the_bundle_ids_exactly() {
        let cx = cx_with_inquiry(5);
        let job = llm::prepare(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL).unwrap();
        let ids: Vec<String> = job.bundle.evidence.iter().map(|e| e.id().to_string()).collect();
        let req = build_request(&job, 0);
        let schema = &req["response_format"]["json_schema"]["schema"];
        let en: Vec<String> = schema["properties"]["sentences"]["items"]["properties"]["evidence"]["items"]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert_eq!(en, ids);
        assert_eq!(req["response_format"]["type"], "json_schema");
        assert_eq!(schema, &response_schema(&job.bundle));
        assert_eq!(schema["properties"]["sentences"]["minItems"], 2);
        assert_eq!(schema["properties"]["sentences"]["maxItems"], 4);
        assert_eq!(schema["properties"]["sentences"]["items"]["properties"]["text"]["minLength"], 15);
        assert_eq!(schema["properties"]["sentences"]["items"]["properties"]["text"]["maxLength"], 140);
        // 교사가 뺀 근거는 enum에도 없다.
        let mut inp = input(&cx, 0, "슬");
        inp.excluded_ids = vec![ids[0].clone()];
        let job2 = llm::prepare(&cx.conn, &cx.media, &inp, MODEL).unwrap();
        let en2 = build_request(&job2, 0)["response_format"]["json_schema"]["schema"]["properties"]["sentences"]["items"]["properties"]["evidence"]["items"]["enum"].clone();
        assert!(!en2.as_array().unwrap().iter().any(|v| v == &json!(ids[0])));
        assert_eq!(en2.as_array().unwrap().len(), ids.len() - 1);
    }

    #[test]
    fn request_options_disable_thinking_and_fix_sampling() {
        let cx = cx_with_inquiry(4);
        let job = llm::prepare(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL).unwrap();
        let req = build_request(&job, 0);
        assert_eq!(req["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(req["temperature"], 0.4);
        assert_eq!(req["max_tokens"], 700);
        assert_eq!(req["stream"], false);
        let msgs = req["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs.len(), 2 + 2 * prompt().examples.len());
        assert_eq!(msgs.last().unwrap()["role"], "user");
        // 예시는 사용자/보조자 쌍으로 들어가고, 보조자 쪽은 JSON이다.
        assert!(serde_json::from_str::<Value>(msgs[2]["content"].as_str().unwrap()).unwrap()["sentences"].is_array());
        // 마지막 사용자 메시지는 이번 묶음의 JSON을 담는다.
        let last = msgs.last().unwrap()["content"].as_str().unwrap();
        assert!(last.contains(&serde_json::to_string(&job.bundle).unwrap()));
    }

    #[test]
    fn seed_comes_from_the_student_id_and_retry_adds_one() {
        let cx = cx_with_inquiry(4);
        let job = llm::prepare(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL).unwrap();
        let a = build_request(&job, 0)["seed"].as_u64().unwrap();
        assert_eq!(a, build_request(&job, 0)["seed"].as_u64().unwrap());
        assert_eq!(build_request(&job, 1)["seed"].as_u64().unwrap(), a + 1);
        assert_eq!(a, seed_for(&cx.ids[0], 0) as u64);
        assert_ne!(seed_for(&cx.ids[0], 0), seed_for(&cx.ids[1], 0));
        assert!(a < u32::MAX as u64);
    }

    #[test]
    fn system_prompt_carries_style_length_and_standards() {
        let cx = cx_with_inquiry(4);
        let job = llm::prepare(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL).unwrap();
        let statement = "학교 안팎의 모습과 생활을 탐색하며 안전한 학교 생활을 한다.";
        assert!(job.bundle.standards_text.values().any(|t| t == statement));
        for (style, s) in [(Style::Ham, "~함."), (Style::Eum, "~하였음."), (Style::Boim, "~하는 모습을 보임.")] {
            let sp = system_prompt("슬", style, 250, &job.bundle.standards_text);
            assert!(sp.contains(s), "{style:?}: {sp}");
            assert!(sp.contains("200~300자") && sp.contains("목표 250자"), "{sp}");
            assert!(sp.contains(statement) && sp.contains("[2슬01-01]"));
            assert!(sp.contains("탐구, 질문, 발견"));
        }
        assert!(system_prompt("바", Style::Ham, 300, &Default::default()).contains("실천, 습관, 관계"));
        assert!(system_prompt("즐", Style::Ham, 300, &Default::default()).contains("놀이, 표현, 협력"));
        // 설정의 문체·글자 수가 요청에 반영된다.
        save_settings(&cx.conn, &EvalSettings { style: "boim".into(), target_chars: 320, extra_forbidden: vec![] }).unwrap();
        let job = llm::prepare(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL).unwrap();
        let sys = build_request(&job, 0)["messages"][0]["content"].as_str().unwrap().to_string();
        assert!(sys.contains("~하는 모습을 보임.") && sys.contains("목표 320자") && sys.contains("256~384자"));
        // 예시도 선택한 문체로 들어간다.
        let ex = build_request(&job, 0)["messages"][2]["content"].as_str().unwrap().to_string();
        assert!(ex.contains("하는 모습을 보임."));
    }

    #[test]
    fn no_student_name_appears_anywhere_in_the_request() {
        let mut cx = seed();
        obs_full(&mut cx, &[0, 1], "2027-04-10", "g1s1-exploration", "김하늘이 박서준이랑 함께 별을 관찰함. 이지우가 도와줌", &["[2슬01-01]"], &["탐구", "질문"]);
        let o = obs_full(&mut cx, &[0], "2027-04-11", "g1s1-school", "하늘이가 서준이와 짝이 되어 돋보기로 관찰함", &["[2슬01-01]"], &["탐구"]);
        let au = attachments::add_bytes(&cx.conn, &cx.media, &o.id, "audio", "audio/webm", b"audio").unwrap();
        attachments::set_transcript(&cx.conn, &cx.media, &au.id, "서준이가 별은 왜 반짝여요? 하고 물었어요. 지우는 몰랐어요").unwrap();
        obs(&mut cx, "2027-04-12", "교실을 살펴봄");
        let job = llm::prepare(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL).unwrap();
        for attempt in 0..2 {
            let body = build_request(&job, attempt).to_string();
            for name in ["김하늘", "박서준", "이지우", "서준", "지우"] {
                assert!(!body.contains(name), "{name}: {body}");
            }
            assert!(body.contains("친구"));
            assert!(body.contains("\\\"alias\\\":\\\"학생\\\""));
        }
    }

    #[test]
    fn good_json_is_saved_with_the_llm_generator() {
        let cx = cx_with_inquiry(4);
        let (out, seen) = run_with(&cx, vec![Ok(good_output(&cx))]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 1);
        assert!(!r.fell_back && r.notice.is_none());
        assert_eq!(r.generator, "llm:qwen3-4b-q4km");
        let d = r.view.draft.unwrap();
        assert_eq!(d.generator, "llm:qwen3-4b-q4km");
        assert_eq!(d.status, "draft");
        assert_eq!(d.sentences.len(), 2);
        assert!(d.sentences[0].text.starts_with("우주 영상"));
        assert!(r.view.check.unwrap().sentences.iter().all(|w| !w.iter().any(|x| x.code == NO_EVIDENCE)));
        // 고쳐 저장해도 만든 방식은 남는다.
        let saved = save_draft(&cx.conn, &cx.media, &SaveInput { key: key(&cx, 0, "슬"), sentences: d.sentences }).unwrap();
        assert_eq!(saved.draft.unwrap().generator, "llm:qwen3-4b-q4km");
    }

    #[test]
    fn think_text_retries_with_next_seed_then_succeeds() {
        let cx = cx_with_inquiry(4);
        let with_think = format!("<think>\n생각\n</think>\n{}", good_output(&cx));
        let (out, seen) = run_with(&cx, vec![Ok(with_think), Ok(good_output(&cx))]);
        let r = out.unwrap();
        assert!(!r.fell_back);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1]["seed"].as_u64().unwrap(), seen[0]["seed"].as_u64().unwrap() + 1);
    }

    #[test]
    fn think_twice_falls_back_to_template_with_notice() {
        let cx = cx_with_inquiry(4);
        let bad = format!("<think>x</think>{}", good_output(&cx));
        let (out, seen) = run_with(&cx, vec![Ok(bad.clone()), Ok(bad)]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 2);
        assert!(r.fell_back);
        assert_eq!(r.notice.as_deref(), Some(FALLBACK_NOTICE));
        assert_eq!(FALLBACK_NOTICE, "언어 모델 초안을 만들지 못해 템플릿 초안을 보여 줍니다");
        assert!(r.detail.unwrap().contains("<think>"));
        assert_eq!(r.generator, "template");
        let d = r.view.draft.unwrap();
        assert_eq!(d.generator, "template");
        assert!(!d.sentences.is_empty());
    }

    #[test]
    fn parse_failure_retries_and_two_failures_fall_back() {
        let cx = cx_with_inquiry(4);
        let (out, seen) = run_with(&cx, vec![Ok("이건 JSON이 아님".into()), Ok(good_output(&cx))]);
        assert!(!out.unwrap().fell_back);
        assert_eq!(seen.len(), 2);

        let cx = cx_with_inquiry(4);
        let (out, seen) = run_with(&cx, vec![Ok("{\"sentences\": []}".into()), Err(LlmError::Failed("서버 응답 오류".into()))]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 2);
        assert!(r.fell_back);
        assert!(r.detail.unwrap().contains("서버 응답 오류"));
        assert_eq!(r.view.draft.unwrap().generator, "template");
    }

    #[test]
    fn sentence_without_valid_evidence_counts_as_a_failure() {
        let cx = cx_with_inquiry(4);
        let no_evidence = json!({ "sentences": [
            { "text": "우주 영상을 보고 궁금한 점을 질문하며 주변 세계를 탐색함.", "evidence": ["o:없는-id"] },
            { "text": "돋보기로 곤충을 자세히 관찰하고 발견한 점을 이야기함.", "evidence": [] },
        ] })
        .to_string();
        let (out, seen) = run_with(&cx, vec![Ok(no_evidence), Ok(good_output(&cx))]);
        assert!(!out.unwrap().fell_back);
        assert_eq!(seen.len(), 2);
        let _ = parse_output("{\"sentences\":[{\"text\":\"  \",\"evidence\":[\"o:a\"]}]}").unwrap_err();
    }

    #[test]
    fn unavailable_server_does_not_retry_and_falls_back() {
        let cx = cx_with_inquiry(4);
        let (out, seen) = run_with(&cx, vec![Err(LlmError::Unavailable("서버를 시작하지 못했습니다".into()))]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 1);
        assert!(r.fell_back);
        assert_eq!(r.view.draft.unwrap().generator, "template");
    }

    #[test]
    fn cancel_stops_without_saving() {
        let cx = cx_with_inquiry(4);
        let (out, seen) = run_with(&cx, vec![Err(LlmError::Cancelled)]);
        assert_eq!(out.unwrap_err(), CANCELLED_MESSAGE);
        assert_eq!(seen.len(), 1);
        assert!(view(&cx.conn, &cx.media, &key(&cx, 0, "슬")).unwrap().draft.is_none());

        // 시작 전에 이미 취소되어 있으면 서버를 부르지 않는다.
        let calls = RefCell::new(0);
        let chat = |_: &Value| -> Result<String, LlmError> {
            *calls.borrow_mut() += 1;
            Ok(String::new())
        };
        let cancel = AtomicBool::new(true);
        assert!(llm::generate_llm_draft(&cx.conn, &cx.media, &input(&cx, 0, "슬"), MODEL, &chat, &cancel).is_err());
        assert_eq!(*calls.borrow(), 0);
    }

    #[test]
    fn too_little_evidence_is_rejected_before_any_request() {
        let cx = cx_with_inquiry(2);
        let (out, seen) = run_with(&cx, vec![Ok(good_output(&cx))]);
        assert!(out.unwrap_err().contains("기록이 부족합니다"));
        assert!(seen.is_empty());
    }

    #[test]
    fn model_setting_defaults_and_rejects_non_llm_ids() {
        let conn = test_conn();
        assert_eq!(llm::load_model_id(&conn).unwrap(), "qwen3-4b-q4km");
        settings::set(&conn, "evalModelId", &json!("qwen2.5-1.5b-q4km")).unwrap();
        assert_eq!(llm::load_model_id(&conn).unwrap(), "qwen2.5-1.5b-q4km");
        settings::set(&conn, "evalModelId", &json!("whisper-base")).unwrap();
        assert_eq!(llm::load_model_id(&conn).unwrap(), "qwen3-4b-q4km");
        settings::set(&conn, "evalModelId", &json!("nope")).unwrap();
        assert_eq!(llm::load_model_id(&conn).unwrap(), "qwen3-4b-q4km");
    }
}

// ---------- 세 교과 통합본 ----------

use super::combined::{self, CombinedKey, CombineInput, CombinedTextInput};

const BA: &str = "인사를 나누고 규칙을 지키는 모습을 보임.";
const SEUL: &str = "돋보기로 곤충을 관찰하며 탐구함.";
const JEUL: &str = "친구와 함께 노래를 부르며 즐겁게 표현함.";

fn seed_all_subjects() -> Cx {
    let mut cx = seed();
    for d in 1..=3 {
        obs_full(&mut cx, &[0], &format!("2027-04-{d:02}"), "g1s1-school", "활동에 참여함", &["[2슬01-01]", "[2바01-01]", "[2즐01-01]"], &["탐구"]);
    }
    cx
}

fn ckey(cx: &Cx, i: usize) -> CombinedKey {
    CombinedKey { student_id: cx.ids[i].clone(), from: FROM.to_string(), to: TO.to_string() }
}

fn confirm_subject(cx: &Cx, i: usize, subject: &str, text: &str) {
    let id = bundle_of(cx, i, subject).evidence[0].id().to_string();
    save_draft(&cx.conn, &cx.media, &SaveInput { key: key(cx, i, subject), sentences: vec![sentence(text, &[&id])] }).unwrap();
    confirm(&cx.conn, &cx.media, &key(cx, i, subject)).unwrap();
}

fn confirm_all(cx: &Cx) {
    confirm_subject(cx, 0, "바", BA);
    confirm_subject(cx, 0, "슬", SEUL);
    confirm_subject(cx, 0, "즐", JEUL);
}

fn do_combine(cx: &Cx, overwrite: bool) -> DbResult<combined::CombinedView> {
    combined::combine(&cx.conn, &CombineInput { key: ckey(cx, 0), overwrite })
}

fn warn_codes(c: &combined::CombinedCheck) -> Vec<&str> {
    c.warnings.iter().map(|w| w.code.as_str()).collect()
}

fn plain_check(cx: &Cx, text: &str) -> combined::CombinedCheck {
    combined::check(&cx.conn, &CombinedTextInput { key: ckey(cx, 0), text: text.to_string() }).unwrap()
}

#[test]
fn combine_refuses_until_all_three_subjects_are_confirmed_and_names_the_missing_ones() {
    let cx = seed_all_subjects();
    let err = do_combine(&cx, false).unwrap_err();
    assert!(err.contains("바른 생활") && err.contains("슬기로운 생활") && err.contains("즐거운 생활"), "{err}");
    confirm_subject(&cx, 0, "바", BA);
    confirm_subject(&cx, 0, "즐", JEUL);
    let err = do_combine(&cx, false).unwrap_err();
    assert!(err.contains("슬기로운 생활 평어를 먼저 확정해 주세요"), "{err}");
    assert!(!err.contains("바른 생활") && !err.contains("즐거운 생활"), "{err}");
    // 초안만 있는 교과는 확정이 아니다.
    let id = bundle_of(&cx, 0, "슬").evidence[0].id().to_string();
    save_draft(&cx.conn, &cx.media, &SaveInput { key: key(&cx, 0, "슬"), sentences: vec![sentence(SEUL, &[&id])] }).unwrap();
    assert!(do_combine(&cx, false).is_err());
    let v = combined::view(&cx.conn, &ckey(&cx, 0)).unwrap();
    assert!(!v.can_combine && v.combined.is_none());
    assert_eq!(v.subjects.iter().map(|s| s.confirmed).collect::<Vec<_>>(), [true, false, true]);
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM evaluation_combined", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn combine_joins_in_order_without_subject_names_or_symbols() {
    let cx = seed_all_subjects();
    // 확정 순서와 상관없이 바 -> 슬 -> 즐
    confirm_subject(&cx, 0, "즐", JEUL);
    confirm_subject(&cx, 0, "슬", SEUL);
    confirm_subject(&cx, 0, "바", BA);
    let v = do_combine(&cx, false).unwrap();
    let c = v.combined.unwrap();
    assert_eq!(c.text, format!("{BA} {SEUL} {JEUL}"));
    assert!(!c.text.contains("바른") && !c.text.contains("슬기") && !c.text.contains(':') && !c.text.contains('\n'));
    assert_eq!((c.status.as_str(), c.stale), ("draft", false));
    assert!(v.can_combine);
    // 학급 현황
    let st = combined::class_combined_status(&cx.conn, &cx.class, FROM, TO).unwrap();
    assert_eq!(st[&cx.ids[0]].status, "draft");
    assert_eq!(st[&cx.ids[1]].status, "none");
}

#[test]
fn combined_edits_are_not_overwritten_without_confirmation() {
    let cx = seed_all_subjects();
    confirm_all(&cx);
    let c = do_combine(&cx, false).unwrap().combined.unwrap();
    combined::save(&cx.conn, &c.id, "선생님이 다듬은 글임.").unwrap();
    let err = do_combine(&cx, false).unwrap_err();
    assert!(err.contains("이미 통합본"), "{err}");
    let again = combined::view(&cx.conn, &ckey(&cx, 0)).unwrap().combined.unwrap();
    assert_eq!(again.text, "선생님이 다듬은 글임.");
    // 덮어쓰기를 허락하면 새로 합친다(같은 행, 초안으로).
    let over = do_combine(&cx, true).unwrap().combined.unwrap();
    assert_eq!(over.id, c.id);
    assert_eq!(over.text, format!("{BA} {SEUL} {JEUL}"));
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM evaluation_combined", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn reconfirming_a_subject_marks_the_combined_text_stale_but_keeps_it() {
    let cx = seed_all_subjects();
    confirm_all(&cx);
    let c = do_combine(&cx, false).unwrap().combined.unwrap();
    combined::save(&cx.conn, &c.id, "선생님이 다듬은 글임.").unwrap();
    combined::confirm(&cx.conn, &c.id).unwrap();
    assert!(!combined::view(&cx.conn, &ckey(&cx, 0)).unwrap().combined.unwrap().stale);
    // 슬 평어를 고쳐 다시 확정
    confirm_subject(&cx, 0, "슬", "돋보기로 개미를 끝까지 관찰함.");
    let v = combined::view(&cx.conn, &ckey(&cx, 0)).unwrap();
    let after = v.combined.unwrap();
    assert!(after.stale);
    assert_eq!(after.text, "선생님이 다듬은 글임.", "통합본 글은 그대로");
    assert_eq!(after.status, "confirmed");
    assert!(combined::class_combined_status(&cx.conn, &cx.class, FROM, TO).unwrap()[&cx.ids[0]].stale);
    // 다시 합치면 해시가 맞춰져 바뀜 표시가 사라진다.
    let again = do_combine(&cx, true).unwrap().combined.unwrap();
    assert!(!again.stale);
    assert!(again.text.contains("개미"));
}

#[test]
fn noun_ending_check_accepts_noun_forms_and_warns_on_other_endings() {
    for ok in [
        "탐구함.",
        "관찰하며 탐구하였음.",
        "친구의 말을 귀 기울여 듣는 모습을 보임.",
        "인사를 잘 나눔.",
        "질문을 이어 감.",
        "차례를 지켜 줌.",
        "끝까지 해냄.",
        "표현함. 탐구함.",
        "마지막 문장에 마침표가 없음",
        "소수 3.5점을 받았음.",
        "표현함!",
        "탐구함\n표현함",
        "따옴표로 끝나는 말을 인용하였음.\")",
    ] {
        assert!(split_ok(ok), "통과해야 함: {ok}");
    }
    for bad in ["탐구했다.", "탐구합니다.", "탐구해요.", "참여하였다", "질문을 했습니다.", "함께 놀이를 하고 있다."] {
        assert!(!split_ok(bad), "경고해야 함: {bad}");
    }
    // 한 문장만 나쁜 경우도 잡는다.
    assert!(!split_ok("탐구함. 열심히 했다. 표현함."));
    // 줄 끝도 문장 끝으로 본다.
    assert!(!split_ok("탐구함\n표현했다"));
}

fn split_ok(text: &str) -> bool {
    combined::split_sentences(text).iter().all(|s| combined::ends_with_noun_form(s))
}

#[test]
fn check_reports_noun_ending_symbols_and_missing_subject() {
    let cx = seed_all_subjects();
    confirm_all(&cx);
    let ok = format!("{BA} {SEUL} {JEUL}");
    let c = plain_check(&cx, &ok);
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert!(c.can_confirm);

    let c = plain_check(&cx, &format!("{BA} {SEUL} 즐겁게 노래를 불렀다."));
    assert!(warn_codes(&c).contains(&combined::NOUN_ENDING), "{:?}", c.warnings);
    assert!(warn_codes(&c).contains(&combined::MISSING_SUBJECT), "{:?}", c.warnings);
    assert!(c.warnings.iter().any(|w| w.message.contains("즐거운 생활 내용이 빠진 것 같습니다")));
    assert!(!c.warnings.iter().any(|w| w.message.contains("바른 생활 내용")));
    assert!(c.can_confirm, "명사형·누락 경고는 확정을 막지 않는다");

    let c = plain_check(&cx, &format!("① {BA}\n- {SEUL}\n• {JEUL} ※"));
    assert!(warn_codes(&c).contains(&combined::SYMBOL), "{:?}", c.warnings);
    let sym = c.warnings.iter().find(|w| w.code == combined::SYMBOL).unwrap();
    assert!(sym.message.contains('①') && sym.message.contains('-') && sym.message.contains('•') && sym.message.contains('※'), "{}", sym.message);
}

#[test]
fn symbol_finder_handles_numbers_bullets_and_emoji_without_false_alarms() {
    for s in ["1. 가나다함.", "2) 가나다함.", "(3) 가나다함.", "* 가나다함.", "※ 가나다함.", "가나다 😀함.", "②"] {
        assert!(!combined::find_symbols(s).is_empty(), "{s}");
    }
    for s in ["1학년 2반 친구와 3월에 활동함.", "1-2학년 활동을 함께함.", "점수 3.5점을 받음.", "가나다함."] {
        assert!(combined::find_symbols(s).is_empty(), "{s}: {:?}", combined::find_symbols(s));
    }
}

#[test]
fn neis_bytes_count_hangul_three_ascii_one_newline_one() {
    assert_eq!(combined::neis_bytes("가나다"), 9);
    assert_eq!(combined::neis_bytes("abc 123"), 7);
    assert_eq!(combined::neis_bytes("가a1\n"), 3 + 1 + 1 + 1);
    assert_eq!(combined::neis_bytes("가\r\n나"), 3 + 1 + 3);
    assert_eq!(combined::neis_bytes("함. 음,"), 3 + 1 + 1 + 3 + 1);
    assert_eq!(combined::count_chars("가a\r\n나"), 4);
    let cx = seed_all_subjects();
    let c = plain_check(&cx, "가나다함.");
    assert_eq!((c.chars, c.bytes), (5, 13));
}

#[test]
fn length_notice_appears_over_500_chars_but_never_blocks() {
    let cx = seed_all_subjects();
    let short = plain_check(&cx, "탐구함.");
    assert!(short.length_notice.is_none());
    let long_text = format!("{}함.", "가".repeat(520));
    let long = plain_check(&cx, &long_text);
    assert!(long.length_notice.as_deref().unwrap().contains("학교 방침에 맞는지 확인하세요"));
    assert!(long.can_confirm);
    assert!(plain_check(&cx, &format!("{}함.", "가".repeat(497))).length_notice.is_none());
}

#[test]
fn forbidden_or_other_student_name_blocks_confirm_and_unconfirmed_text_is_not_exportable() {
    let cx = seed_all_subjects();
    confirm_all(&cx);
    let c = do_combine(&cx, false).unwrap().combined.unwrap();

    combined::save(&cx.conn, &c.id, "박서준과 함께 탐구함.").unwrap();
    let err = combined::confirm(&cx.conn, &c.id).unwrap_err();
    assert!(err.contains("확정할 수 없습니다"), "{err}");
    let ck = plain_check(&cx, "박서준과 함께 탐구함.");
    assert!(warn_codes(&ck).contains(&OTHER_NAME) && !ck.can_confirm);

    let bad = forbidden().find("우리 반 1등", &[]);
    assert!(!bad.is_empty(), "테스트용 금지 표현이 기본 목록에 있어야 한다");
    combined::save(&cx.conn, &c.id, "우리 반 1등 하였음.").unwrap();
    assert!(combined::confirm(&cx.conn, &c.id).is_err());
    let ck = plain_check(&cx, "우리 반 1등 하였음.");
    assert!(warn_codes(&ck).contains(&FORBIDDEN) && !ck.can_confirm);

    // 확정 전에는 내보내기에 나오지 않는다.
    assert!(combined::export_text(&cx.conn, &cx.class, FROM, TO).unwrap_err().contains("확정된 통합본이 없습니다"));
    combined::save(&cx.conn, &c.id, "함께 탐구함.").unwrap();
    assert!(combined::export_text(&cx.conn, &cx.class, FROM, TO).is_err());
    let v = combined::confirm(&cx.conn, &c.id).unwrap();
    assert_eq!(v.combined.unwrap().status, "confirmed");
    assert!(combined::export_text(&cx.conn, &cx.class, FROM, TO).is_ok());
    // 고치면 다시 초안이 된다.
    let v = combined::save(&cx.conn, &c.id, "함께 탐구하였음.").unwrap();
    assert_eq!(v.combined.unwrap().status, "draft");
    assert!(combined::save(&cx.conn, &c.id, "   ").is_err());
    assert!(combined::save(&cx.conn, "없는-id", "글").unwrap_err().contains("찾을 수 없습니다"));
    assert_eq!(combined::unconfirm(&cx.conn, &c.id).unwrap().combined.unwrap().status, "draft");
}

#[test]
fn export_has_two_formats_and_only_confirmed_rows() {
    let cx = seed_all_subjects();
    confirm_all(&cx);
    let c = do_combine(&cx, false).unwrap().combined.unwrap();
    // 통합본이 초안이면 통합본 내보내기는 없고 교과별은 된다.
    assert!(export_by_format(&cx.conn, &cx.class, FROM, TO, None).unwrap_err().contains("통합본"));
    let (by_subject, n) = export_by_format(&cx.conn, &cx.class, FROM, TO, Some("bySubject")).unwrap();
    assert_eq!(n, 3);
    assert!(by_subject.starts_with("번호\t이름\t교과\t평어\r\n"));
    combined::confirm(&cx.conn, &c.id).unwrap();
    let (text, n) = export_by_format(&cx.conn, &cx.class, FROM, TO, None).unwrap();
    assert_eq!(n, 1);
    assert_eq!(text, format!("번호\t이름\t성취수준 및 특기사항\r\n1\t김하늘\t{BA} {SEUL} {JEUL}\r\n"));
    assert_eq!(export_by_format(&cx.conn, &cx.class, FROM, TO, Some("combined")).unwrap().0, text);
    assert!(export_by_format(&cx.conn, &cx.class, FROM, TO, Some("xml")).is_err());
}

#[test]
fn stored_target_chars_are_kept_when_the_default_changes() {
    let cx = seed();
    assert_eq!(load_settings(&cx.conn).unwrap().target_chars, 150);
    settings::set(&cx.conn, KEY_TARGET, &serde_json::json!(250)).unwrap();
    assert_eq!(load_settings(&cx.conn).unwrap().target_chars, 250);
    settings::set(&cx.conn, KEY_TARGET, &serde_json::json!(100)).unwrap();
    assert_eq!(load_settings(&cx.conn).unwrap().target_chars, 100);
}

#[test]
fn deleting_a_student_removes_the_combined_text() {
    let cx = seed_all_subjects();
    confirm_all(&cx);
    do_combine(&cx, false).unwrap();
    cx.conn.execute("DELETE FROM students WHERE id = ?1", [&cx.ids[0]]).unwrap();
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM evaluation_combined", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}
