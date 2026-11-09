//! 행동특성 및 종합의견(설계 24) 테스트.

use std::cell::RefCell;
use std::sync::atomic::AtomicBool;

use serde_json::{json, Value};

use super::behavior::{self, build_bundle, check_behavior, generate_draft, GenerateInput, OpinionKey, SaveInput, NO_CHANGE, NO_STRENGTH};
use super::bundle::{Bundle, Evidence};
use super::check::{FORBIDDEN, MISMATCH, NO_EVIDENCE, OTHER_NAME};
use super::combined::{ends_with_noun_form, NOUN_ENDING, SYMBOL};
use super::data::{behavior_templates, forbidden};
use super::generate::{fill, Sentence, Style};
use super::tests::{obs_full, seed, sentence, Cx};
use crate::db::observations::{self, create, ObservationInput};
use crate::db::{students, DbResult};
use crate::llm_server::LlmError;

/// 1학기와 2학기를 함께 담는 기간.
const BF: &str = "2027-03-02";
const BT: &str = "2028-02-28";

fn bkey(cx: &Cx, i: usize) -> OpinionKey {
    OpinionKey { student_id: cx.ids[i].clone(), from: BF.to_string(), to: BT.to_string() }
}

fn ginput(cx: &Cx, i: usize, split: bool) -> GenerateInput {
    GenerateInput { key: bkey(cx, i), excluded_ids: vec![], split_semesters: split }
}

/// 하늘(0)의 관찰. `negative`는 「지도가 필요한 모습」 표시.
fn bobs(cx: &mut Cx, day: &str, topic: &str, note: &str, codes: &[&str], tags: &[&str], negative: bool) -> observations::Observation {
    let input = ObservationInput {
        class_id: cx.class.clone(),
        observed_at: format!("{day}T10:00:00+09:00"),
        topic_book_id: if topic.is_empty() { None } else { Some(topic.to_string()) },
        note: note.to_string(),
        student_ids: vec![cx.ids[0].clone()],
        standard_codes: codes.iter().map(|c| c.to_string()).collect(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        unit_session_id: None,
        needs_guidance: negative,
    };
    create(&mut cx.conn, &cx.media, &input).unwrap()
}

fn bundle(cx: &Cx, i: usize) -> Bundle {
    build_bundle(&cx.conn, &cx.media, &cx.ids[i], BF, BT, &[]).unwrap()
}

/// 장점 넷, 지도가 필요한 모습 하나(그 뒤에 같은 태그의 긍정 기록이 이어져 짝이 된다). 모두 1학기.
fn seed_pair(cx: &mut Cx) -> (String, String) {
    bobs(cx, "2027-04-10", "g1s1-exploration", "친구가 넘어지자 손을 잡아 일으켜 줌", &["[2바01-01]"], &["배려"], false);
    bobs(cx, "2027-04-20", "g1s1-school", "청소 시간에 맡은 구역을 끝까지 정리함", &["[2슬01-01]"], &["책임"], false);
    bobs(cx, "2027-05-10", "g1s1-school", "모둠 친구들과 역할을 나누어 게시판을 꾸밈", &[], &["협력"], false);
    let n = bobs(cx, "2027-05-20", "g1s1-school", "차례를 지키지 않고 친구를 밀침", &[], &["책임"], true);
    let p = bobs(cx, "2027-06-14", "g1s1-exploration", "줄을 서서 차례를 기다리고 친구에게 양보함", &[], &["책임"], false);
    (format!("o:{}", n.id), format!("o:{}", p.id))
}

fn obs_of(b: &Bundle, id: &str) -> Evidence {
    b.evidence.iter().find(|e| e.id() == id).cloned().unwrap()
}

fn codes(w: &[super::check::Warning]) -> Vec<&str> {
    w.iter().map(|x| x.code.as_str()).collect()
}

fn bad_codes(c: &behavior::BehaviorCheck) -> Vec<String> {
    c.sentences.iter().flatten().chain(c.overall.iter()).map(|w| w.code.clone()).collect()
}

// ---------- 근거 묶음 ----------

#[test]
fn bundle_collects_all_subjects_flags_negative_and_pairs_the_later_positive() {
    let mut cx = seed();
    let (n_id, p_id) = seed_pair(&mut cx);
    // 다른 학생만의 기록, 삭제된 기록, 기간 밖 기록, 빈 기록은 들어가지 않는다.
    obs_full(&mut cx, &[1], "2027-04-02", "g1s1-school", "서준 혼자 한 일", &[], &["배려"]);
    let gone = bobs(&mut cx, "2027-04-03", "g1s1-school", "삭제될 기록", &[], &["배려"], false);
    observations::delete(&cx.conn, &gone.id).unwrap();
    bobs(&mut cx, "2028-05-03", "g1s1-school", "기간 밖", &[], &["배려"], false);
    bobs(&mut cx, "2027-04-04", "g1s1-school", "", &[], &["배려"], false);

    let b = bundle(&cx, 0);
    assert_eq!(b.subject, "행");
    assert_eq!(b.evidence.len(), 5, "교과와 상관없이 그 아이의 기간 안 기록 전체");
    assert!(b.standards_text.is_empty());
    assert!(obs_of(&b, &n_id).is_negative());
    match obs_of(&b, &p_id) {
        Evidence::Observation { pair, negative, .. } => assert_eq!((pair, negative), (Some(n_id.clone()), false)),
        e => panic!("{e:?}"),
    }
    let json = serde_json::to_string(&b).unwrap();
    assert_eq!(json.matches("\"negative\":true").count(), 1, "negative는 해당 기록에만, 거짓이면 필드 자체가 없다");
    assert_eq!(json.matches("\"pair\":").count(), 1);
    // 다른 태그의 긍정 기록은 짝이 되지 않는다.
    assert!(b.evidence.iter().filter(|e| matches!(e, Evidence::Observation { pair: Some(_), .. })).count() == 1);
}

#[test]
fn a_later_positive_with_a_different_tag_is_not_a_pair_and_an_earlier_one_is_not_either() {
    let mut cx = seed();
    bobs(&mut cx, "2027-04-01", "g1s1-school", "먼저 한 좋은 일", &[], &["책임"], false);
    let n = bobs(&mut cx, "2027-05-01", "g1s1-school", "차례를 지키지 않고 친구를 밀침", &[], &["책임"], true);
    bobs(&mut cx, "2027-06-01", "g1s1-school", "다른 태그의 좋은 일", &[], &["안전"], false);
    bobs(&mut cx, "2027-06-02", "g1s1-school", "또 다른 좋은 일", &[], &["배려"], false);
    let b = bundle(&cx, 0);
    assert!(b.evidence.iter().all(|e| !matches!(e, Evidence::Observation { pair: Some(_), .. })));
    assert!(obs_of(&b, &format!("o:{}", n.id)).is_negative());
}

#[test]
fn selection_prefers_life_tags_then_guidance_then_the_rest_and_caps_at_14() {
    let mut cx = seed();
    // 인성·생활 태그 12개, 지도가 필요한 모습 2개(짝 없음), 그 밖 5개.
    for i in 0..12 {
        let topic = if i % 2 == 0 { "g1s1-school" } else { "g1s1-exploration" };
        bobs(&mut cx, &format!("2027-04-{:02}", i + 1), topic, &format!("친구를 도운 일{i}"), &[], &["협력"], false);
    }
    for i in 0..2 {
        bobs(&mut cx, &format!("2027-05-{:02}", i + 1), "g1s1-school", &format!("지도가 필요한 일{i}"), &[], &["놀이"], true);
    }
    for i in 0..5 {
        bobs(&mut cx, &format!("2027-06-{:02}", i + 1), "g1s1-school", &format!("그 밖의 일{i}"), &[], &["탐구"], false);
    }
    let b = bundle(&cx, 0);
    assert_eq!(b.evidence.len(), 14);
    assert_eq!(b.evidence.iter().filter(|e| e.is_negative()).count(), 2, "지도가 필요한 모습이 그 밖보다 먼저");
    let rest = b.evidence.iter().filter(|e| matches!(e, Evidence::Observation { tags, .. } if tags == &vec!["탐구".to_string()])).count();
    assert_eq!(rest, 0);
    // 인성·생활 태그가 14개를 넘으면 지도가 필요한 모습은 밀린다.
    for i in 0..4 {
        bobs(&mut cx, &format!("2027-07-{:02}", i + 1), "g1s1-school", &format!("더 도운 일{i}"), &[], &["배려"], false);
    }
    let b = bundle(&cx, 0);
    assert_eq!(b.evidence.len(), 14);
    assert_eq!(b.evidence.iter().filter(|e| e.is_negative()).count(), 0);
    // 주제책이 고르게 섞인다.
    let topics: std::collections::HashSet<_> = b.evidence.iter().filter_map(|e| if let Evidence::Observation { topic, .. } = e { topic.clone() } else { None }).collect();
    assert!(topics.len() >= 2);
}

#[test]
fn a_pair_stays_in_the_bundle_even_when_life_tag_records_fill_the_cap() {
    let mut cx = seed();
    for i in 0..16 {
        bobs(&mut cx, &format!("2027-04-{:02}", i + 1), "g1s1-school", &format!("친구를 도운 일{i}"), &[], &["협력"], false);
    }
    let n = bobs(&mut cx, "2027-05-05", "g1s1-school", "차례를 지키지 않고 친구를 밀침", &[], &["책임"], true);
    let p = bobs(&mut cx, "2027-06-05", "g1s1-school", "줄을 서서 차례를 기다림", &[], &["책임"], false);
    let b = bundle(&cx, 0);
    assert_eq!(b.evidence.len(), 14);
    let ids: Vec<&str> = b.evidence.iter().map(|e| e.id()).collect();
    assert!(ids.contains(&format!("o:{}", n.id).as_str()) && ids.contains(&format!("o:{}", p.id).as_str()), "변화 근거(짝)는 함께 들어간다");
}

#[test]
fn fewer_than_three_evidence_refuses_and_saves_nothing() {
    let mut cx = seed();
    bobs(&mut cx, "2027-04-10", "g1s1-school", "친구를 도와줌", &[], &["배려"], false);
    bobs(&mut cx, "2027-04-11", "g1s1-school", "청소를 끝까지 함", &[], &["책임"], false);
    assert_eq!(bundle(&cx, 0).evidence.len(), 2);
    let err = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap_err();
    assert!(err.contains("기록이 부족합니다(2개)"), "{err}");
    let n: i64 = cx.conn.query_row("SELECT count(*) FROM behavior_opinions", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    // 체크를 풀어 3개 미만이 되어도 만들지 않는다.
    bobs(&mut cx, "2027-04-12", "g1s1-school", "블록을 정리함", &[], &["습관"], false);
    let b = bundle(&cx, 0);
    let mut input = ginput(&cx, 0, false);
    input.excluded_ids = vec![b.evidence[0].id().to_string()];
    assert!(generate_draft(&cx.conn, &cx.media, &input).unwrap_err().contains("기록이 부족합니다(2개)"));
    assert!(generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).is_ok());
}

#[test]
fn other_students_names_never_reach_the_bundle_and_own_name_becomes_the_alias() {
    let mut cx = seed();
    obs_full(&mut cx, &[0, 1], "2027-04-10", "g1s1-school", "김하늘이 박서준이랑 함께 정리함. 이지우가 도와줌", &[], &["협력"]);
    bobs(&mut cx, "2027-04-11", "g1s1-school", "서준이가 넘어지자 일으켜 줌", &[], &["배려"], false);
    bobs(&mut cx, "2027-04-12", "g1s1-school", "지우와 다투는 일이 있었음", &[], &["존중"], true);
    bobs(&mut cx, "2027-04-13", "g1s1-school", "블록을 정리함", &[], &["책임"], false);
    let b = bundle(&cx, 0);
    let json = serde_json::to_string(&b).unwrap();
    for name in ["박서준", "서준", "이지우", "지우", "김하늘"] {
        assert!(!json.contains(name), "{name}: {json}");
    }
    assert!(json.contains("친구") && json.contains("\"alias\":\"학생\""));
    // 만들어진 초안과 화면 묶음에도 없다.
    let v = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap();
    let all = serde_json::to_string(&v).unwrap();
    for name in ["박서준", "서준", "이지우", "지우"] {
        assert!(!all.contains(name), "{name}");
    }
}

// ---------- 템플릿 생성 ----------

#[test]
fn template_draft_is_deterministic_cites_real_evidence_and_passes_every_check() {
    let mut cx = seed();
    let (n_id, p_id) = seed_pair(&mut cx);
    let first = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap();
    let a = first.opinion.clone().unwrap();
    assert_eq!((a.status.as_str(), a.generator.as_str(), a.split_semesters), ("draft", "template", false));
    let second = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap();
    assert_eq!(second.opinion.unwrap().sentences, a.sentences, "같은 입력이면 같은 초안");

    let ids: Vec<&str> = first.bundle.evidence.iter().map(|e| e.id()).collect();
    assert!(a.sentences.len() >= 3);
    for s in &a.sentences {
        assert!(!s.evidence.is_empty() && s.evidence.iter().all(|e| ids.contains(&e.as_str())), "{s:?}");
        assert!(!s.reviewed, "만든 직후에는 확인 전");
    }
    // 짝(지도가 필요한 모습 -> 변화)은 두 근거를 한 문장으로 묶고, 변화 표현이 있다.
    let growth = a.sentences.iter().find(|s| s.evidence.contains(&n_id)).expect("짝 문장");
    assert_eq!(growth.evidence, [n_id.clone(), p_id.clone()]);
    assert!(behavior::has_change_word(&growth.text), "{}", growth.text);
    assert!(growth.text.contains("밀치는"), "근거 메모가 문장에 들어간다: {}", growth.text);
    assert!(a.sentences.iter().any(|s| !s.evidence.contains(&n_id)), "장점 문장도 있다");
    let bad = bad_codes(first.check.as_ref().unwrap());
    assert!(bad.is_empty(), "{bad:?}: {:?}", first.check);
    assert!(first.check.unwrap().can_confirm);
}

#[test]
fn an_unpaired_guidance_record_gets_a_hopeful_sentence_after_the_strengths() {
    let mut cx = seed();
    bobs(&mut cx, "2027-04-10", "g1s1-exploration", "친구가 넘어지자 손을 잡아 일으켜 줌", &[], &["배려"], false);
    bobs(&mut cx, "2027-04-20", "g1s1-school", "청소 시간에 맡은 구역을 끝까지 정리함", &[], &["책임"], false);
    let n = bobs(&mut cx, "2027-05-20", "g1s1-school", "놀이 시간에 정리를 미루고 놀이를 계속함", &[], &["습관"], true);
    let v = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap();
    let s = v.opinion.unwrap().sentences;
    let last = s.last().unwrap();
    assert_eq!(last.evidence, [format!("o:{}", n.id)], "단점 문장은 장점 뒤에");
    assert!(behavior::has_change_word(&last.text) && last.text.contains("정리를 미루고 놀이를 계속하는"), "{}", last.text);
    assert!(s.len() >= 3 && s[..s.len() - 1].iter().all(|x| !x.evidence.contains(&last.evidence[0])));
    assert!(bad_codes(&v.check.unwrap()).is_empty());
}

#[test]
fn guidance_records_alone_never_produce_a_draft() {
    let mut cx = seed();
    for i in 0..3 {
        bobs(&mut cx, &format!("2027-04-{:02}", i + 10), "g1s1-school", &format!("차례를 지키지 않고 친구를 밀침{}", ["", "", ""][i]), &[], &["놀이"], true);
    }
    let err = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap_err();
    assert!(err.contains("초안을 만들 수 없습니다"), "{err}");
}

#[test]
fn split_semesters_make_two_chunks_with_labels_that_the_symbol_check_allows() {
    let mut cx = seed();
    seed_pair(&mut cx);
    bobs(&mut cx, "2027-09-12", "g1s1-school", "계단에서 뛰지 않고 천천히 걸어 내려감", &[], &["안전"], false);
    bobs(&mut cx, "2027-10-05", "g1s1-exploration", "친구의 이야기를 끝까지 들어 줌", &[], &["존중"], false);
    bobs(&mut cx, "2027-11-05", "g1s1-school", "화단에 물을 주고 정리함", &[], &["책임"], false);
    let v = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, true)).unwrap();
    let o = v.opinion.unwrap();
    assert!(o.split_semesters);
    let labelled: Vec<usize> = o.sentences.iter().enumerate().filter(|(_, s)| s.text.starts_with("(1학기) ") || s.text.starts_with("(2학기) ")).map(|(i, _)| i).collect();
    assert_eq!(labelled.len(), 2, "{:?}", o.sentences);
    assert_eq!(labelled[0], 0);
    assert!(o.sentences[0].text.starts_with("(1학기) "));
    assert!(o.sentences[labelled[1]].text.starts_with("(2학기) "));
    // 2학기 덩어리에는 2학기 기록만, 1학기 덩어리에는 1학기 기록만 쓴다.
    let date_of = |id: &str| if let Evidence::Observation { date, .. } = obs_of(&v.bundle, id) { date } else { String::new() };
    for (i, s) in o.sentences.iter().enumerate() {
        for e in &s.evidence {
            let sem2 = date_of(e).as_str() >= "2027-09-01";
            assert_eq!(sem2, i >= labelled[1], "{i}번째 문장은 다른 학기의 근거를 가리킴");
        }
    }
    assert!(bad_codes(&v.check.unwrap()).is_empty(), "(1학기) 표기는 특수문자 검사에서 예외");
    // 나누지 않으면 학기 표시가 없다.
    let plain = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap().opinion.unwrap();
    assert!(plain.sentences.iter().all(|s| !s.text.starts_with('(')) && !plain.split_semesters);
}

#[test]
fn every_behavior_template_is_clean_in_every_style() {
    let t = behavior_templates();
    let sample = |n: &str| match n {
        "note" => Some("친구를 도와주는".to_string()),
        "before" => Some("차례를 지키지 않고 끼어드는".to_string()),
        "topic" => Some("학교".to_string()),
        _ => None,
    };
    assert!(t.templates.len() >= 30);
    for tpl in &t.templates {
        assert!(tpl.text.contains("{note}"), "{}: 근거 메모를 써야 함", tpl.id);
        assert!(!tpl.text.contains("모습"), "{}", tpl.id);
        if tpl.kind == "growth" {
            assert!(tpl.text.contains("{before}"), "{}", tpl.id);
        }
        for style in [Style::Ham, Style::Eum, Style::Boim] {
            let s = fill(&tpl.text, style.ending(), &sample);
            assert!(!s.contains('{') && !s.contains('}'), "{}: {s}", tpl.id);
            assert!(ends_with_noun_form(&s), "{}: {s}", tpl.id);
            assert!(forbidden().find(&s, &[]).is_empty(), "{}: 금지 표현 {s}", tpl.id);
            if tpl.kind != "strength" {
                assert!(behavior::has_change_word(&s), "{}: 변화 가능성 표현이 없음 {s}", tpl.id);
            }
            if tpl.kind == "concern" {
                assert!(!tpl.text.contains("{E}"), "{}: 단점 틀은 고정 종결", tpl.id);
            }
        }
    }
    for kind in ["strength", "growth", "concern"] {
        assert!(t.templates.iter().filter(|x| x.kind == kind).count() >= 4, "{kind}");
    }
    assert_eq!(t.semester_labels, ["(1학기)", "(2학기)"]);
}

// ---------- 검사 ----------

fn check_of(cx: &Cx, b: &Bundle, sentences: &[Sentence]) -> behavior::BehaviorCheck {
    check_behavior(sentences, b, &[], &super::other_names(&cx.conn, &cx.ids[0]).unwrap())
}

#[test]
fn check_warns_when_a_guidance_sentence_has_no_change_or_when_only_guidance_is_written() {
    let mut cx = seed();
    let (n_id, p_id) = seed_pair(&mut cx);
    let b = bundle(&cx, 0);
    let plain = sentence("차례를 지키지 않고 친구를 밀치는 때가 있음.", &[&n_id]);
    let c = check_of(&cx, &b, std::slice::from_ref(&plain));
    assert!(codes(&c.sentences[0]).contains(&NO_CHANGE), "{:?}", c.sentences);
    assert!(c.sentences[0].iter().any(|w| w.message == "단점을 쓸 때는 변화 가능성을 함께 써 주세요(기재요령)"));
    assert!(c.overall.iter().any(|w| w.code == NO_STRENGTH), "단점 문장만 있으면 경고");

    let hopeful = sentence("차례를 지키지 않고 친구를 밀치는 때가 있으나 꾸준히 연습한다면 나아질 것으로 기대됨.", &[&n_id]);
    let c = check_of(&cx, &b, std::slice::from_ref(&hopeful));
    assert!(!codes(&c.sentences[0]).contains(&NO_CHANGE));
    assert!(c.overall.iter().any(|w| w.code == NO_STRENGTH), "변화 표현이 있어도 장점이 없으면 경고");
    let strength = sentence("줄을 서서 차례를 기다리고 친구에게 양보함.", &[&p_id]);
    let c = check_of(&cx, &b, &[strength.clone(), hopeful]);
    assert!(c.sentences.iter().flatten().all(|w| w.code != NO_CHANGE) && c.overall.is_empty(), "{c:?}");
    // 단점이 아닌 문장은 변화 표현이 없어도 된다.
    assert!(!codes(&check_of(&cx, &b, &[strength]).sentences[0]).contains(&NO_CHANGE));
}

#[test]
fn check_reuses_the_shared_checks_and_lets_semester_labels_and_lmieum_endings_through() {
    let mut cx = seed();
    let (_, p_id) = seed_pair(&mut cx);
    let b = bundle(&cx, 0);
    let ok = check_of(&cx, &b, &[sentence("(1학기) 줄을 서서 차례를 기다리며 분위기를 만듦.", &[&p_id]), sentence("(2학기) 친구에게 양보하며 사는 법을 앎.", &[&p_id])]);
    let bad = bad_codes(&ok);
    assert!(!bad.iter().any(|c| c == SYMBOL || c == NOUN_ENDING), "{bad:?}");
    // 명사형이 아닌 끝, 번호·특수문자, 금지 표현, 다른 학생 이름, 근거 없음.
    let c = check_of(&cx, &b, &[sentence("줄을 서서 기다렸다.", &[&p_id])]);
    assert!(codes(&c.sentences[0]).contains(&NOUN_ENDING));
    let c = check_of(&cx, &b, &[sentence("1. 줄을 서서 기다림.", &[&p_id])]);
    assert!(codes(&c.sentences[0]).contains(&SYMBOL));
    let c = check_of(&cx, &b, &[sentence("★ 줄을 서서 기다림.", &[&p_id])]);
    assert!(codes(&c.sentences[0]).contains(&SYMBOL));
    let c = check_of(&cx, &b, &[sentence("반에서 1등으로 줄을 섬.", &[&p_id])]);
    assert!(codes(&c.sentences[0]).contains(&FORBIDDEN));
    let c = check_of(&cx, &b, &[sentence("박서준과 함께 줄을 섬.", &[&p_id])]);
    assert!(codes(&c.sentences[0]).contains(&OTHER_NAME));
    let c = check_of(&cx, &b, &[sentence("줄을 섬.", &[])]);
    assert!(codes(&c.sentences[0]).contains(&NO_EVIDENCE) && !c.can_confirm);
    let c = check_of(&cx, &b, &[sentence("우주선을 타고 화성에서 돌아옴.", &[&p_id])]);
    assert!(codes(&c.sentences[0]).contains(&MISMATCH));
    // 글자 수와 NEIS 바이트(한글 3, 영문·숫자 1).
    let c = check_of(&cx, &b, &[sentence("가나다 abc 12함.", &[&p_id])]);
    assert_eq!(c.total_chars, 12);
    assert_eq!(c.total_bytes, 3 * 3 + 1 + 3 + 1 + 2 + 3 + 1);
    assert!(c.length.is_some(), "너무 짧으면 안내");
}

// ---------- 확인 장치와 저장 ----------

#[test]
fn confirm_needs_every_sentence_reviewed_and_edits_keep_the_checks() {
    let mut cx = seed();
    seed_pair(&mut cx);
    let k = bkey(&cx, 0);
    let v = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap();
    let o = v.opinion.unwrap();
    assert!(!v.check.unwrap().all_reviewed);
    let err = behavior::confirm(&cx.conn, &cx.media, &k).unwrap_err();
    assert!(err.contains("1번째 문장의 「근거 확인」"), "{err}");
    // 일부만 확인
    let mut sentences = o.sentences.clone();
    sentences[0].reviewed = true;
    let save = |sentences: Vec<Sentence>| behavior::save_draft(&cx.conn, &cx.media, &SaveInput { key: k.clone(), sentences, split_semesters: false }).unwrap();
    save(sentences.clone());
    let err = behavior::confirm(&cx.conn, &cx.media, &k).unwrap_err();
    assert!(err.contains("2번째 문장의 「근거 확인」"), "{err}");
    // 문장을 고쳐도 체크는 저장된 그대로, 새 문장은 체크된 채로 더할 수 있다.
    sentences[0].text.push_str(" 이어 감.");
    sentences.iter_mut().for_each(|s| s.reviewed = true);
    let ev = sentences[0].evidence[0].clone();
    sentences.push(sentence("직접 덧붙인 문장함.", &[&ev]));
    let v = save(sentences);
    assert_eq!(v.opinion.as_ref().unwrap().generator, "template", "고쳐 저장해도 만든 방식은 남는다");
    let c = behavior::confirm(&cx.conn, &cx.media, &k).unwrap();
    let o = c.opinion.unwrap();
    assert_eq!(o.status, "confirmed");
    assert!(o.confirmed_at.is_some() && o.sentences.iter().all(|s| s.reviewed));
    assert!(!o.unchanged_from_model);
    // 고치면 다시 초안이 된다.
    let again = save(o.sentences.clone()).opinion.unwrap();
    assert_eq!((again.status.as_str(), again.confirmed_at), ("draft", None));
    // 문장이 없거나 근거가 없으면 확정할 수 없다.
    save(vec![sentence("근거 없는 문장함.", &[])]);
    assert!(behavior::confirm(&cx.conn, &cx.media, &k).unwrap_err().contains("근거가 없습니다"));
    assert!(behavior::save_draft(&cx.conn, &cx.media, &SaveInput { key: k.clone(), sentences: vec![sentence("  ", &[&ev])], split_semesters: false }).is_err());
}

#[test]
fn target_chars_default_300_and_range_150_to_800() {
    let cx = seed();
    assert_eq!(behavior::load_target(&cx.conn).unwrap(), 300);
    assert_eq!(behavior::save_target(&cx.conn, 150).unwrap(), 150);
    assert_eq!(behavior::save_target(&cx.conn, 800).unwrap(), 800);
    assert!(behavior::save_target(&cx.conn, 149).is_err() && behavior::save_target(&cx.conn, 801).is_err());
    assert_eq!(behavior::load_target(&cx.conn).unwrap(), 800);
}

#[test]
fn view_status_export_and_cascade() {
    let mut cx = seed();
    seed_pair(&mut cx);
    obs_full(&mut cx, &[1], "2027-04-01", "g1s1-school", "서준 기록", &[], &["배려"]);
    let k = bkey(&cx, 0);
    let before = behavior::view(&cx.conn, &cx.media, &k).unwrap();
    assert!(before.enough && before.opinion.is_none() && before.check.is_none());
    assert_eq!((before.target_chars, before.student.number), (300, 1));
    assert!(behavior::export_text(&cx.conn, &cx.class, BF, BT).unwrap_err().contains("확정된 행동특성 및 종합의견이 없습니다"));
    let rows = behavior::class_status(&cx.conn, &cx.media, &cx.class, BF, BT).unwrap();
    assert_eq!(rows.iter().map(|r| (r.number, r.evidence_count, r.status.as_str())).collect::<Vec<_>>(), [(1, 5, "none"), (2, 1, "none"), (3, 0, "none")]);

    let v = generate_draft(&cx.conn, &cx.media, &ginput(&cx, 0, false)).unwrap();
    assert_eq!(behavior::class_status(&cx.conn, &cx.media, &cx.class, BF, BT).unwrap()[0].status, "draft");
    assert!(behavior::export_text(&cx.conn, &cx.class, BF, BT).is_err(), "확정 전에는 내보내지 않는다");
    let mut sentences = v.opinion.unwrap().sentences;
    sentences.iter_mut().for_each(|s| s.reviewed = true);
    sentences[0].text = "여럿이\t함께\n정리함.".to_string();
    behavior::save_draft(&cx.conn, &cx.media, &SaveInput { key: k.clone(), sentences: sentences.clone(), split_semesters: false }).unwrap();
    behavior::confirm(&cx.conn, &cx.media, &k).unwrap();
    assert_eq!(behavior::class_status(&cx.conn, &cx.media, &cx.class, BF, BT).unwrap()[0].status, "confirmed");
    let (text, n) = behavior::export_text(&cx.conn, &cx.class, BF, BT).unwrap();
    assert_eq!(n, 1);
    let want = sentences.iter().map(|s| s.text.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" ");
    assert_eq!(text, format!("번호\t이름\t행동특성 및 종합의견\r\n1\t김하늘\t{want}\r\n"));
    assert!(text.starts_with("번호\t이름\t행동특성 및 종합의견\r\n1\t김하늘\t여럿이 함께 정리함."));
    // eval_export의 형식 이름으로도 나온다.
    assert_eq!(super::export_by_format(&cx.conn, &cx.class, BF, BT, Some("behavior")).unwrap().0, text);
    // 학생을 지우면 함께 지워진다.
    cx.conn.execute("DELETE FROM students WHERE id = ?1", [&cx.ids[0]]).unwrap();
    let left: i64 = cx.conn.query_row("SELECT count(*) FROM behavior_opinions", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
    assert!(students::list(&cx.conn, &cx.class, true).unwrap().len() == 2);
}

#[test]
fn needs_guidance_roundtrips_through_create_and_update() {
    let mut cx = seed();
    let o = bobs(&mut cx, "2027-04-10", "g1s1-school", "정리를 미룸", &[], &["습관"], true);
    assert!(o.needs_guidance);
    let input = ObservationInput {
        class_id: cx.class.clone(),
        observed_at: o.observed_at.clone(),
        topic_book_id: o.topic_book_id.clone(),
        note: o.note.clone(),
        student_ids: o.student_ids.clone(),
        standard_codes: vec![],
        tags: o.tags.clone(),
        unit_session_id: None,
        needs_guidance: false,
    };
    let u = observations::update(&mut cx.conn, &cx.media, &o.id, &input).unwrap();
    assert!(!u.needs_guidance);
    assert!(!observations::get(&cx.conn, &cx.media, &o.id).unwrap().needs_guidance);
    // 주제책 없이 남기는 생활 기록도 근거가 된다.
    bobs(&mut cx, "2027-04-11", "", "친구를 도와줌", &[], &["배려"], false);
    let b = bundle(&cx, 0);
    assert!(b.evidence.iter().any(|e| matches!(e, Evidence::Observation { topic: None, .. })));
}

// ---------- 언어 모델 ----------

mod llm {
    use super::*;
    use crate::db::evaluation::behavior_llm::{self as bl, prompt, prepare, request_body, response_schema, system_prompt};
    use crate::db::evaluation::llm::FALLBACK_NOTICE;

    const MODEL: &str = "qwen3-4b-q4km";

    fn run_with(cx: &Cx, split: bool, replies: Vec<Result<String, LlmError>>) -> (DbResult<bl::BehaviorLlmResult>, Vec<Value>) {
        let seen = RefCell::new(Vec::<Value>::new());
        let queue = RefCell::new(replies.into_iter());
        let chat = |req: &Value| -> Result<String, LlmError> {
            seen.borrow_mut().push(req.clone());
            queue.borrow_mut().next().unwrap_or_else(|| Err(LlmError::Failed("no more".into())))
        };
        let cancel = AtomicBool::new(false);
        let out = bl::generate_llm_draft(&cx.conn, &cx.media, &ginput(cx, 0, split), MODEL, &chat, &cancel);
        (out, seen.into_inner())
    }

    fn reply(sentences: &[(&str, &[&str])]) -> String {
        json!({ "sentences": sentences.iter().map(|(t, e)| json!({ "text": t, "evidence": e })).collect::<Vec<_>>() }).to_string()
    }

    fn good(cx: &Cx, n_id: &str, p_id: &str) -> String {
        let b = bundle(cx, 0);
        let first = b.evidence.iter().map(|e| e.id()).find(|i| *i != n_id && *i != p_id).unwrap().to_string();
        reply(&[
            ("친구를 도우며 배려하는 마음을 실천함.", &[first.as_str()]),
            ("차례를 지키지 않고 친구를 밀치는 때가 있었으나 이후 줄을 서서 기다리며 점차 성장함.", &[n_id, p_id]),
        ])
    }

    #[test]
    fn prompt_json_examples_pass_the_behavior_checks_in_every_style() {
        let p = prompt();
        assert!((2..=3).contains(&p.examples.len()));
        for ex in &p.examples {
            let ids: Vec<&str> = ex.bundle.evidence.iter().map(|e| e.id()).collect();
            assert_eq!(ex.bundle.subject, "행");
            assert!(ex.bundle.evidence.iter().any(|e| e.is_negative()), "{}: negative 근거 예시", ex.id);
            for style in ["ham", "eum", "boim"] {
                let sentences = &ex.outputs[style];
                for s in sentences {
                    assert!(s.evidence.iter().all(|e| ids.contains(&e.as_str())));
                    assert!(!s.text.contains("학생"));
                }
                let c = check_behavior(sentences, &ex.bundle, &[], &[]);
                assert!(c.sentences.iter().all(|w| w.is_empty()), "{} {style}: {:?}", ex.id, c.sentences);
                assert!(c.overall.is_empty() && c.length.is_none(), "{} {style}: {c:?}", ex.id);
                assert!(c.can_confirm);
            }
        }
    }

    #[test]
    fn request_has_schema_enum_of_bundle_ids_no_names_and_behavior_prompt() {
        let mut cx = seed();
        let (n, p) = seed_pair(&mut cx);
        obs_full(&mut cx, &[0, 1], "2027-04-02", "g1s1-school", "박서준과 함께 정리함", &[], &["협력"]);
        let job = prepare(&cx.conn, &cx.media, &ginput(&cx, 0, false), MODEL).unwrap();
        let b = &job.chunks[0].bundle;
        let req = request_body(b, Style::Ham, 300, 7, 2);
        let ids: Vec<Value> = b.evidence.iter().map(|e| json!(e.id())).collect();
        assert_eq!(req["response_format"]["json_schema"]["schema"]["properties"]["sentences"]["items"]["properties"]["evidence"]["items"]["enum"], Value::Array(ids));
        assert_eq!(req["chat_template_kwargs"]["enable_thinking"], false);
        let text = req.to_string();
        for name in ["박서준", "서준", "이지우", "김하늘"] {
            assert!(!text.contains(name), "{name}");
        }
        let user = req["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
        assert!(user.contains(&n) && user.contains(&p) && user.contains("\"negative\":true") && user.contains("\"pair\""), "{user}");
        assert!(!text.contains("reviewed"), "확인 표시는 모델에게 보이지 않는다");
        let sys = system_prompt(Style::Boim, 400);
        assert!(sys.contains("행동특성 및 종합의견") && sys.contains("320~480") && sys.contains("하는 모습을 보임"), "{sys}");
        assert!(sys.contains("변화 가능성"));
        // 예시 대화는 설계 24의 데이터 파일에서 온다(평어 프롬프트가 아니다).
        assert_eq!(req["messages"].as_array().unwrap().len(), 1 + 2 * prompt().examples.len() + 1);
    }

    #[test]
    fn good_json_is_saved_as_llm_and_flagged_until_the_teacher_edits() {
        let mut cx = seed();
        let (n, p) = seed_pair(&mut cx);
        let (out, seen) = run_with(&cx, false, vec![Ok(good(&cx, &n, &p))]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 1);
        assert!(!r.fell_back && r.notice.is_none());
        let o = r.view.opinion.unwrap();
        assert_eq!((o.generator.as_str(), o.status.as_str()), ("llm:qwen3-4b-q4km", "draft"));
        assert!(o.unchanged_from_model, "한 글자도 고치지 않았다");
        assert!(o.sentences.iter().all(|s| !s.reviewed));
        // 확인만 하고 확정하려 하면 백엔드는 막지 않고, 화면이 물을 수 있도록 플래그를 유지한다.
        let k = bkey(&cx, 0);
        let mut sentences = o.sentences.clone();
        sentences.iter_mut().for_each(|s| s.reviewed = true);
        let v = behavior::save_draft(&cx.conn, &cx.media, &SaveInput { key: k.clone(), sentences: sentences.clone(), split_semesters: false }).unwrap();
        assert!(v.opinion.as_ref().unwrap().unchanged_from_model);
        let c = behavior::confirm(&cx.conn, &cx.media, &k).unwrap().opinion.unwrap();
        assert!(c.status == "confirmed" && c.unchanged_from_model);
        sentences[0].text = "친구를 도우며 배려하는 마음을 실천하려고 노력함.".to_string();
        let edited = behavior::save_draft(&cx.conn, &cx.media, &SaveInput { key: k, sentences, split_semesters: false }).unwrap().opinion.unwrap();
        assert!(!edited.unchanged_from_model);
        assert_eq!(edited.generator, "llm:qwen3-4b-q4km");
    }

    #[test]
    fn a_guidance_sentence_without_change_words_counts_as_a_failure_and_falls_back_after_two_tries() {
        let mut cx = seed();
        let (n, p) = seed_pair(&mut cx);
        let b = bundle(&cx, 0);
        let first = b.evidence.iter().map(|e| e.id()).find(|i| *i != n && *i != p).unwrap().to_string();
        let bad = reply(&[("친구를 도우며 배려하는 마음을 실천함.", &[first.as_str()]), ("차례를 지키지 않고 친구를 밀치는 때가 있음.", &[n.as_str()])]);
        let (out, seen) = run_with(&cx, false, vec![Ok(bad.clone()), Ok(bad)]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1]["seed"].as_u64().unwrap(), seen[0]["seed"].as_u64().unwrap() + 1);
        assert!(r.fell_back && r.notice.as_deref() == Some(FALLBACK_NOTICE));
        assert_eq!(r.generator, "template");
        let o = r.view.opinion.unwrap();
        assert_eq!(o.generator, "template");
        assert!(!o.unchanged_from_model);
        // 두 번째에 좋은 결과가 오면 쓴다.
        let (out, seen) = run_with(&cx, false, vec![Ok("{\"sentences\": []}".into()), Ok(good(&cx, &n, &p))]);
        assert!(!out.unwrap().fell_back && seen.len() == 2);
    }

    #[test]
    fn unavailable_server_falls_back_without_retry_and_cancel_saves_nothing() {
        let mut cx = seed();
        seed_pair(&mut cx);
        let (out, seen) = run_with(&cx, false, vec![Err(LlmError::Unavailable("서버 없음".into()))]);
        let r = out.unwrap();
        assert!(r.fell_back && seen.len() == 1 && r.detail.as_deref() == Some("서버 없음"));
        let n: i64 = cx.conn.query_row("SELECT count(*) FROM behavior_opinions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "대신 만든 템플릿 초안");
        cx.conn.execute("DELETE FROM behavior_opinions", []).unwrap();
        let (out, _) = run_with(&cx, false, vec![Err(LlmError::Cancelled)]);
        assert!(out.is_err());
        let n: i64 = cx.conn.query_row("SELECT count(*) FROM behavior_opinions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn split_semesters_call_the_server_per_semester_and_label_each_chunk() {
        let mut cx = seed();
        seed_pair(&mut cx);
        let s2a = bobs(&mut cx, "2027-09-12", "g1s1-school", "계단에서 뛰지 않고 천천히 걸어 내려감", &[], &["안전"], false);
        let s2b = bobs(&mut cx, "2027-10-05", "g1s1-exploration", "친구의 이야기를 끝까지 들어 줌", &[], &["존중"], false);
        let b = bundle(&cx, 0);
        let sem1 = b.evidence.iter().map(|e| e.id()).find(|i| i.starts_with("o:") && !i.contains(&s2a.id) && !i.contains(&s2b.id)).unwrap().to_string();
        let r1 = reply(&[("친구가 넘어지자 손을 잡아 일으켜 주며 배려함.", &[sem1.as_str()])]);
        let r2 = reply(&[("계단에서 천천히 걸으며 안전 수칙을 지킴.", &[&format!("o:{}", s2a.id)]), ("(2학기) 친구의 이야기를 끝까지 들으며 존중함.", &[&format!("o:{}", s2b.id)])]);
        let (out, seen) = run_with(&cx, true, vec![Ok(r1), Ok(r2)]);
        let r = out.unwrap();
        assert_eq!(seen.len(), 2, "학기마다 한 번씩");
        // 각 요청에는 그 학기의 근거만 들어 있다.
        let user1 = seen[0]["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
        let user2 = seen[1]["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
        assert!(user1.contains("2027-04-10") && !user1.contains("2027-09-12"));
        assert!(user2.contains("2027-09-12") && !user2.contains("2027-04-10"));
        let o = r.view.opinion.unwrap();
        assert!(o.split_semesters);
        assert_eq!(o.sentences.iter().filter(|s| s.text.starts_with("(1학기) ")).count(), 1);
        assert_eq!(o.sentences.iter().filter(|s| s.text.starts_with("(2학기) ")).count(), 1, "모델이 붙인 학기 표시는 지우고 덩어리 앞에만 붙인다");
        assert!(o.sentences[0].text.starts_with("(1학기) 친구가"));
        let chk = r.view.check.unwrap();
        assert!(bad_codes(&chk).is_empty(), "{:?} {:?}", chk.sentences, o.sentences);
    }

    #[test]
    fn schema_enum_follows_the_chunk_and_too_little_evidence_is_rejected_before_any_request() {
        let mut cx = seed();
        bobs(&mut cx, "2027-04-10", "g1s1-school", "친구를 도와줌", &[], &["배려"], false);
        bobs(&mut cx, "2027-04-11", "g1s1-school", "청소를 끝까지 함", &[], &["책임"], false);
        let (out, seen) = run_with(&cx, false, vec![]);
        assert!(out.unwrap_err().contains("기록이 부족합니다"));
        assert!(seen.is_empty());
        let b = bundle(&cx, 0);
        let schema = response_schema(&b, 2);
        assert_eq!(schema["properties"]["sentences"]["minItems"], 2);
    }
}
