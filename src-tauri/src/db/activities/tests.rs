use super::*;
use crate::db::units::{self, NextSession, SessionInput, SessionStandard, UnitCreateInput, UnitDetail};
use crate::db::{classes, test_conn};

const TOPIC: &str = "g1s1-exploration";

fn card(id: &str, subject: &str, kind: &str, keywords: &[&str]) -> Activity {
    let std = |s: &str| format!("[2{s}02-04]");
    let mut standards = vec![std(subject)];
    if subject != "슬" {
        standards.push(std("슬"));
    }
    Activity {
        id: id.to_string(),
        topic_book_id: TOPIC.to_string(),
        title: format!("{id} 활동 카드"),
        subject: subject.to_string(),
        standards,
        kind: kind.to_string(),
        minutes: 40,
        steps: vec!["처음 모여서 이야기를 나눈다".into(), "직접 해 보며 살펴본다".into(), "알게 된 것을 정리한다".into()],
        materials: vec!["돋보기".into()],
        keywords: keywords.iter().map(|k| k.to_string()).collect(),
        makingbook: None,
    }
}

fn new_unit(conn: &mut Connection, topic: Option<&str>) -> UnitDetail {
    let c = classes::create(conn, 2027, 1, "1반").unwrap();
    units::create(
        conn,
        &UnitCreateInput {
            class_id: c.id,
            topic_book_id: topic.map(String::from),
            tool_id: "free".into(),
            title: "탐험 단원".into(),
            stage_titles: vec!["수업".into()],
        },
    )
    .unwrap()
}

fn session(unit: &UnitDetail, title: &str) -> SessionInput {
    SessionInput {
        unit_id: unit.id.clone(),
        stage_id: unit.stages[0].id.clone(),
        title: title.into(),
        plan: String::new(),
        minutes: 40,
        kind: "general".into(),
        scheduled_on: None,
        status: "planned".into(),
        student_questions: String::new(),
        reflection: String::new(),
        standards: vec![],
        activity_id: None,
    }
}

fn ask(conn: &Connection, unit: &UnitDetail, cards: &[Activity], questions: &str) -> Vec<Recommendation> {
    recommend_with(conn, cards, &RecommendInput { unit_id: unit.id.clone(), after_session_id: None, questions: Some(questions.into()) }).unwrap()
}

fn ids(r: &[Recommendation]) -> Vec<&str> {
    r.iter().map(|x| x.activity.id.as_str()).collect()
}

fn by_id<'a>(r: &'a [Recommendation], id: &str) -> &'a Recommendation {
    r.iter().find(|x| x.activity.id == id).unwrap_or_else(|| panic!("{id} 없음: {:?}", ids(r)))
}

// ───────── 카드 ─────────

#[test]
fn builtin_cards_are_all_valid_with_unique_ids() {
    let mut seen = HashSet::new();
    for a in builtin() {
        assert!(seen.insert(a.id.clone()), "id 중복 {}", a.id);
        let v = validate(a).unwrap_or_else(|e| panic!("{}: {e}", a.id));
        assert_eq!(&v, a, "{}는 다듬은 모양이어야 합니다", a.id);
    }
    assert!(!builtin().is_empty());
}

#[test]
fn custom_card_save_update_delete() {
    let conn = test_conn();
    assert!(list_custom(&conn).unwrap().is_empty());
    let saved = save_custom(&conn, &card("", "슬", "general", &["개미", "곤충", "관찰"])).unwrap();
    assert!(saved.id.starts_with("custom-"));
    let mut edit = saved.clone();
    edit.title = "  개미 집 탐험하기 ".into();
    edit.minutes = 80;
    let updated = save_custom(&conn, &edit).unwrap();
    assert_eq!((updated.id.as_str(), updated.title.as_str(), updated.minutes), (saved.id.as_str(), "개미 집 탐험하기", 80));
    assert_eq!(list_custom(&conn).unwrap(), vec![updated.clone()]);
    assert_eq!(find(&conn, &saved.id).unwrap(), updated);
    delete_custom(&conn, &saved.id).unwrap();
    assert!(list_custom(&conn).unwrap().is_empty());
    assert!(delete_custom(&conn, &saved.id).is_err());
    assert!(find(&conn, &saved.id).is_err());
}

#[test]
fn invalid_custom_cards_are_rejected() {
    let conn = test_conn();
    let ok = card("", "슬", "general", &["개미", "곤충", "관찰"]);
    let bad = |f: &dyn Fn(&mut Activity)| {
        let mut a = ok.clone();
        f(&mut a);
        save_custom(&conn, &a)
    };
    // 다른 주제책(학교)의 성취기준
    let e = bad(&|a| a.standards = vec!["[2슬01-01]".into()]).unwrap_err();
    assert!(e.contains("성취기준이 아닙니다"), "{e}");
    assert!(bad(&|a| a.topic_book_id = "g1s1-nothing".into()).is_err());
    assert!(bad(&|a| a.standards = vec![]).is_err());
    assert!(bad(&|a| a.standards = vec!["[2슬02-04]".into(), "[2슬02-04]".into()]).is_err());
    // 첫째 성취기준이 교과와 다름
    assert!(bad(&|a| a.standards = vec!["[2바02-04]".into()]).is_err());
    assert!(bad(&|a| a.subject = "수".into()).is_err());
    assert!(bad(&|a| a.kind = "play".into()).is_err());
    assert!(bad(&|a| a.minutes = 60).is_err());
    assert!(bad(&|a| a.title = "짧음".into()).is_err());
    assert!(bad(&|a| a.steps.truncate(2)).is_err());
    assert!(bad(&|a| a.steps[0] = "짧다".into()).is_err());
    assert!(bad(&|a| a.keywords.truncate(2)).is_err());
    assert!(bad(&|a| a.materials = vec!["가".repeat(21)]).is_err());
    assert!(bad(&|a| a.id = builtin().first().map(|b| b.id.clone()).unwrap_or_else(|| "x".into())).unwrap_err().contains("기본 카드"));
    assert!(bad(&|a| a.id = "custom-none".into()).is_err());
    assert!(list_custom(&conn).unwrap().is_empty());
}

// ───────── 추천 ─────────

#[test]
fn question_words_score_with_reasons_and_cap() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let mut ant = card("ant", "슬", "general", &["개미", "곤충", "관찰"]);
    ant.title = "개미 집 찾아보기".into();
    let cards = vec![
        ant,
        card("bug", "슬", "general", &["곤충", "나비", "관찰"]),
        card("many", "슬", "general", &["개미", "나비", "벌레", "꽃"]),
        card("none", "슬", "general", &["바위", "구름", "하늘"]),
    ];
    let r = ask(&conn, &unit, &cards, "개미는 어디에 살아요? 나비도 곤충이에요?");
    // 낱말: 개미(조사 '는' 뗌), 나비도 -> 나비, 곤충이에요 -> 곤충
    assert_eq!(by_id(&r, "ant").reasons[0], "학생 질문의 '개미'와 이어져요");
    let many = cards.iter().find(|c| c.id == "many").unwrap();
    assert_eq!(matched_words("개미는 어디에 살아요? 나비도 곤충이에요?", many), ["개미", "나비"]);
    // 같은 낱말은 한 번만 세고, 최대 3개(+9)
    assert_eq!(matched_words("개미 나비 벌레가 꽃에 개미", &cards[2]), ["개미", "나비", "벌레"]);
    // 제목에 있는 낱말도 센다. 한 글자 낱말은 세지 않는다.
    assert_eq!(matched_words("집 찾아보기가 재밌어요", &cards[0]), ["찾아보기"]);
    // 질문이 없으면 낱말 점수 없음
    let none = ask(&conn, &unit, &cards, "   ");
    assert!(by_id(&none, "ant").reasons.iter().all(|x| !x.starts_with("학생 질문")));
    // ant: 낱말 개미(+3) 곤충(+3) + 성취기준 2개? (슬 1개만 -> +2)
    assert_eq!(by_id(&r, "ant").score, 3 + 3 + 2);
}

#[test]
fn uncovered_standards_add_points_and_covered_ones_do_not() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let cards = vec![card("sl", "슬", "general", &["가나", "다라", "마바"]), card("ba", "바", "general", &["가나", "다라", "마바"])];
    let r = ask(&conn, &unit, &cards, "");
    // ba: [2바02-04] + [2슬02-04] -> +4, sl: [2슬02-04] -> +2
    assert_eq!(ids(&r), ["ba", "sl"]);
    assert_eq!((by_id(&r, "ba").score, by_id(&r, "sl").score), (4, 2));
    assert!(by_id(&r, "ba").reasons.contains(&"[2바02-04]를 아직 다루지 않았어요".to_string()));
    // 슬 성취기준을 한 차시에서 다루면 그 기준의 가점은 사라진다.
    let mut s = session(&unit, "차시 하나");
    s.standards = vec![SessionStandard { code: "[2슬02-04]".into(), mode: "repeat".into() }];
    let mut conn2 = conn;
    units::create_session(&mut conn2, &s).unwrap();
    let r = ask(&conn2, &unit, &cards, "");
    assert_eq!((by_id(&r, "ba").score, by_id(&r, "sl").score), (2, 0));
    assert!(by_id(&r, "sl").reasons.is_empty());
}

#[test]
fn lacking_safety_and_makingbook_sessions_add_points() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let cards = vec![
        card("a-safe", "슬", "safety", &["가나", "다라", "마바"]),
        card("b-make", "슬", "makingbook", &["가나", "다라", "마바"]),
        card("c-gen", "슬", "general", &["가나", "다라", "마바"]),
    ];
    let r = ask(&conn, &unit, &cards, "");
    // 모두 슬 성취기준 +2, 안전·메이킹북은 +3
    assert_eq!((by_id(&r, "a-safe").score, by_id(&r, "b-make").score, by_id(&r, "c-gen").score), (5, 5, 2));
    assert!(by_id(&r, "a-safe").reasons.contains(&"안전 더하기가 4차시 더 필요해요".to_string()));
    assert!(by_id(&r, "b-make").reasons.contains(&"메이킹북이 3차시 더 필요해요".to_string()));
    // 안전 80분(2차시)을 채우면 "2차시 더", 4차시를 채우면 가점이 없다.
    let mut s = session(&unit, "안전 차시");
    s.kind = "safety".into();
    s.minutes = 80;
    units::create_session(&mut conn, &s).unwrap();
    let r = ask(&conn, &unit, &cards, "");
    assert!(by_id(&r, "a-safe").reasons.contains(&"안전 더하기가 2차시 더 필요해요".to_string()));
    let mut s2 = session(&unit, "안전 차시 둘");
    s2.kind = "safety".into();
    s2.minutes = 80;
    units::create_session(&mut conn, &s2).unwrap();
    let r = ask(&conn, &unit, &cards, "");
    assert_eq!(by_id(&r, "a-safe").score, 2);
    // 건너뛴 차시는 채운 것으로 치지 않는다.
    let mut skipped = session(&unit, "건너뜀");
    skipped.kind = "makingbook".into();
    skipped.status = "skipped".into();
    skipped.minutes = 120;
    units::create_session(&mut conn, &skipped).unwrap();
    assert_eq!(by_id(&ask(&conn, &unit, &cards, ""), "b-make").score, 5);
}

#[test]
fn used_cards_are_excluded_and_session_keeps_activity_id() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let saved = save_custom(&conn, &card("", "슬", "general", &["개미", "곤충", "관찰"])).unwrap();
    let cards = vec![card("one", "슬", "general", &["가나", "다라", "마바"])];
    let mut s = session(&unit, "첫 차시");
    s.activity_id = Some("one".into());
    let made = units::create_session(&mut conn, &s).unwrap();
    assert_eq!(made.activity_id.as_deref(), Some("one"));
    assert_eq!(units::get_session(&conn, &made.id).unwrap().activity_id.as_deref(), Some("one"));
    let r = ask(&conn, &unit, &cards, "");
    assert_eq!(ids(&r), [saved.id.as_str()]);
    // 교사 카드도 쓰면 빠진다.
    let mut s2 = session(&unit, "둘째");
    s2.activity_id = Some(saved.id.clone());
    units::create_session(&mut conn, &s2).unwrap();
    assert!(ask(&conn, &unit, &cards, "").is_empty());
    // 수정해도 activity_id가 유지된다(프런트가 그대로 보낸다).
    let mut edit = s.clone();
    edit.title = "고친 제목".into();
    let after = units::update_session(&mut conn, &made.id, &edit).unwrap();
    assert_eq!(after.activity_id.as_deref(), Some("one"));
}

#[test]
fn order_is_deterministic_and_limited_to_three() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let cards: Vec<Activity> = ["e", "b", "d", "a", "c"].iter().map(|id| card(id, "슬", "general", &["가나", "다라", "마바"])).collect();
    let r = ask(&conn, &unit, &cards, "");
    assert_eq!(ids(&r), ["a", "b", "c"]);
    let mut reversed = cards.clone();
    reversed.reverse();
    assert_eq!(ids(&ask(&conn, &unit, &reversed, "")), ["a", "b", "c"]);
}

#[test]
fn variety_bonus_and_questions_from_the_previous_session() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let cards = vec![card("ba", "바", "general", &["가나", "다라", "마바"]), card("sl", "슬", "general", &["개미", "다라", "마바"])];
    let mut s = session(&unit, "앞 차시");
    s.standards = vec![SessionStandard { code: "[2슬02-04]".into(), mode: "focus".into() }, SessionStandard { code: "[2바02-04]".into(), mode: "focus".into() }];
    s.student_questions = "개미는 어디서 자요?".into();
    s.activity_id = Some("prev".into());
    let prev = units::create_session(&mut conn, &s).unwrap();
    let input = RecommendInput { unit_id: unit.id.clone(), after_session_id: Some(prev.id.clone()), questions: None };
    let r = recommend_with(&conn, &cards, &input).unwrap();
    // 앞 차시 성취기준이 이미 모두 다뤄짐: 질문 낱말('개미') 일치만 점수가 있다. 앞 차시의 교과(슬·바)와 겹치므로 교과 가점은 없다.
    assert_eq!(by_id(&r, "sl").score, 3);
    assert_eq!(by_id(&r, "ba").score, 0);
    // 카드로 만든 앞 차시는 그 카드의 교과로 비교한다.
    let prev_card = save_custom(&conn, &card("", "즐", "general", &["고래", "바다", "물결"])).unwrap();
    let mut s2 = session(&unit, "즐 차시");
    s2.activity_id = Some(prev_card.id.clone());
    let p2 = units::create_session(&mut conn, &s2).unwrap();
    let input = RecommendInput { unit_id: unit.id.clone(), after_session_id: Some(p2.id), questions: Some("고래".into()) };
    let r = recommend_with(&conn, &cards, &input).unwrap();
    // 즐 다음이므로 바·슬 모두 +1(교과 고르게). 질문 '고래'는 이 카드들에 없다.
    assert_eq!((by_id(&r, "ba").score, by_id(&r, "sl").score), (1, 1));
    // 입력한 질문이 앞 차시 질문보다 우선한다.
    let input = RecommendInput { unit_id: unit.id.clone(), after_session_id: Some(prev.id), questions: Some("다라".into()) };
    let r = recommend_with(&conn, &cards, &input).unwrap();
    assert!(by_id(&r, "ba").reasons.contains(&"학생 질문의 '다라'와 이어져요".to_string()));
    // 다른 단원의 차시는 거부
    let other = new_unit(&mut conn, Some(TOPIC));
    let bad = RecommendInput { unit_id: other.id, after_session_id: Some("none".into()), questions: None };
    assert!(recommend_with(&conn, &cards, &bad).is_err());
}

#[test]
fn unit_without_topic_book_uses_every_card_of_the_grade() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, None);
    let mut other_topic = card("g1-day", "슬", "general", &["가나", "다라", "마바"]);
    other_topic.topic_book_id = "g1s2-day".into();
    let mut grade2 = card("g2-me", "슬", "general", &["가나", "다라", "마바"]);
    grade2.topic_book_id = "g2s1-me".into();
    let cards = vec![card("g1-exp", "슬", "general", &["가나", "다라", "마바"]), other_topic, grade2];
    let r = ask(&conn, &unit, &cards, "");
    assert_eq!(ids(&r), ["g1-day", "g1-exp"]);
    // 주제책이 있는 단원은 그 주제책 카드만
    let with_topic = new_unit(&mut conn, Some("g1s2-day"));
    assert_eq!(ids(&ask(&conn, &with_topic, &cards, "")), ["g1-day"]);
}

#[test]
fn follow_up_with_a_card_fills_the_new_session_and_saves_activity_id() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let first = units::create_session(&mut conn, &session(&unit, "첫 차시")).unwrap();
    let mut c = card("", "슬", "safety", &["개미", "곤충", "관찰"]);
    c.minutes = 80;
    let saved = save_custom(&conn, &c).unwrap();
    let next = NextSession { title: "개미 집 탐험".into(), stage_id: unit.stages[0].id.clone(), activity_id: Some(saved.id.clone()) };
    let detail = units::complete_and_follow(&mut conn, &first.id, "개미는 어디 살아요?", "", Some(&next)).unwrap();
    let made = &detail.stages[0].sessions[1];
    assert_eq!(made.activity_id.as_deref(), Some(saved.id.as_str()));
    assert_eq!((made.minutes, made.kind.as_str()), (80, "safety"));
    assert_eq!(made.standards.iter().map(|s| (s.code.as_str(), s.mode.as_str())).collect::<Vec<_>>(), [("[2슬02-04]", "focus")]);
    assert!(made.plan.starts_with("앞 차시 학생 질문:\n개미는 어디 살아요?\n\n1. 처음 모여서 이야기를 나눈다\n2. "), "{}", made.plan);
    assert!(made.plan.ends_with("준비물: 돋보기"), "{}", made.plan);
    // 없는 카드면 완료 처리도 되돌려진다.
    let second = units::create_session(&mut conn, &session(&unit, "둘째")).unwrap();
    let bad = NextSession { title: "z".into(), stage_id: unit.stages[0].id.clone(), activity_id: Some("custom-none".into()) };
    assert!(units::complete_and_follow(&mut conn, &second.id, "", "", Some(&bad)).is_err());
    assert_eq!(units::get_session(&conn, &second.id).unwrap().status, "planned");
}

/// 설계 문서와 보고서에 싣는 추천 예시.
#[test]
fn recommendation_examples() {
    let mut conn = test_conn();
    let unit = new_unit(&mut conn, Some(TOPIC));
    let mut ant = card("ant-house", "슬", "general", &["개미", "곤충", "관찰"]);
    ant.title = "개미 집 살펴보기".into();
    let mut safe = card("trail-safety", "바", "safety", &["안전", "산책", "길"]);
    safe.title = "탐험길 안전 약속".into();
    let mut make = card("make-book", "즐", "makingbook", &["책", "그림", "만들기"]);
    make.title = "탐험 메이킹북 만들기".into();
    let cards = vec![ant, safe, make, card("sky", "슬", "general", &["하늘", "구름", "바람"])];
    let show = |label: &str, r: &[Recommendation]| {
        println!("== {label}");
        for x in r {
            println!("  {} (점수 {}): {}", x.activity.title, x.score, x.reasons.join(" / "));
        }
    };
    let mut s = session(&unit, "숲길 걷기");
    s.standards = vec![SessionStandard { code: "[2슬02-04]".into(), mode: "focus".into() }];
    s.kind = "safety".into();
    s.minutes = 80;
    s.student_questions = "안전하게 산책하고 싶어요. 어디까지 가요?".into();
    let done = units::create_session(&mut conn, &s).unwrap();
    let r1 = recommend_with(&conn, &cards, &RecommendInput { unit_id: unit.id.clone(), after_session_id: Some(done.id.clone()), questions: Some("개미가 곤충이래요. 집을 관찰해요".into()) }).unwrap();
    show("예시 1: 질문 '개미가 곤충이래요. 집을 관찰해요'", &r1);
    assert_eq!(r1[0].activity.id, "ant-house");
    assert_eq!(r1[0].score, 9);
    assert_eq!(r1[0].reasons, ["학생 질문의 '개미'와 이어져요", "학생 질문의 '곤충'과 이어져요", "학생 질문의 '관찰'과 이어져요"]);
    let r2 = recommend_with(&conn, &cards, &RecommendInput { unit_id: unit.id.clone(), after_session_id: Some(done.id), questions: None }).unwrap();
    show("예시 2: 질문 입력 없이 앞 차시(안전 80분, 슬 기준 다룸)의 학생 질문 사용", &r2);
    assert_eq!(r2[0].activity.id, "trail-safety");
}

fn make_card(template: &str, pages: usize) -> Activity {
    let mut c = card("", "즐", "makingbook", &["책", "그림", "만들기"]);
    c.makingbook = Some(Makingbook { template: template.into(), title: "우리 학교".into(), pages: (0..pages).map(|i| format!("{}번째 칸을 그려요", i + 1)).collect() });
    c
}

#[test]
fn custom_makingbook_template_is_saved_and_read_back() {
    let conn = test_conn();
    for (template, pages) in [("zine8", 6), ("card2", 2), ("accordion4", 4)] {
        let saved = save_custom(&conn, &make_card(template, pages)).unwrap();
        let found = find(&conn, &saved.id).unwrap();
        assert_eq!(found.makingbook.as_ref().map(|m| (m.template.as_str(), m.pages.len())), Some((template, pages)));
    }
    // 틀이 없는 메이킹북 카드도 저장할 수 있다.
    assert!(save_custom(&conn, &card("", "즐", "makingbook", &["책", "그림", "만들기"])).unwrap().makingbook.is_none());
    // 공백은 다듬는다.
    let mut c = make_card("card2", 2);
    c.makingbook.as_mut().unwrap().title = "  내 책  ".into();
    assert_eq!(save_custom(&conn, &c).unwrap().makingbook.unwrap().title, "내 책");
}

#[test]
fn invalid_makingbook_templates_are_rejected() {
    let conn = test_conn();
    // 틀별 안내 개수
    for (template, pages) in [("zine8", 5), ("zine8", 7), ("card2", 3), ("accordion4", 2)] {
        let err = save_custom(&conn, &make_card(template, pages)).unwrap_err();
        assert!(err.contains("안내가"), "{template} {pages}: {err}");
    }
    // 메이킹북이 아닌 카드
    let mut c = make_card("zine8", 6);
    c.kind = "general".into();
    assert!(save_custom(&conn, &c).unwrap_err().contains("메이킹북 카드에만"));
    // 모르는 틀, 길이 제한
    assert!(save_custom(&conn, &make_card("book9", 6)).is_err());
    let mut c = make_card("card2", 2);
    c.makingbook.as_mut().unwrap().title = "가".into();
    assert!(save_custom(&conn, &c).is_err());
    let mut c = make_card("card2", 2);
    c.makingbook.as_mut().unwrap().title = "가".repeat(17);
    assert!(save_custom(&conn, &c).is_err());
    let mut c = make_card("card2", 2);
    c.makingbook.as_mut().unwrap().pages[1] = "가".repeat(25);
    assert!(save_custom(&conn, &c).is_err());
    let mut c = make_card("card2", 2);
    c.makingbook.as_mut().unwrap().pages[0] = " 가 ".into();
    assert!(save_custom(&conn, &c).is_err());
    assert!(list_custom(&conn).unwrap().is_empty());
}

#[test]
fn builtin_makingbook_cards_all_have_a_template() {
    let mb: Vec<_> = builtin().iter().filter(|a| a.kind == "makingbook").collect();
    assert_eq!(mb.len(), 16);
    for a in &mb {
        assert!(a.makingbook.is_some(), "{} 메이킹북 카드에 인쇄 틀이 없음", a.id);
    }
    assert!(builtin().iter().filter(|a| a.kind != "makingbook").all(|a| a.makingbook.is_none()));
}
