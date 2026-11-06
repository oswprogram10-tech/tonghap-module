//! 활동 추천의 뜻 점수(설계 22). 학생 질문과 카드 글의 임베딩 코사인으로 같은 주제책 카드의 순위를 매긴다.
//!
//! 카드 벡터는 `embeddings` 표에 저장해 두고(카드 글의 해시가 바뀌면 다시 계산), 학생 질문 벡터는 저장하지 않는다.
//! 임베딩 서버 호출은 DB 잠금 밖에서 하도록 `plan`(잠금 안) -> `Plan::compute`(잠금 밖) -> `store_and_build`(잠금 안)으로 나눴다.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, Connection};

use super::Activity;
use crate::db::{DbExt, DbResult};
use crate::embed::{self, EmbedFn};

const KIND_CARD: &str = "card";
/// 같은 주제책 카드 안에서 뜻이 가까운 순서대로 더하는 점수(1등, 2등, 3등).
/// e5는 관련 없는 글끼리도 코사인이 0.7대라 절댓값 문턱은 쓰지 않고 순위만 믿는다.
const RANK_POINTS: [i64; 3] = [4, 3, 2];
const MAX_QUERIES: usize = 30;
const REASON_QUESTION_CHARS: usize = 20;

/// 질문 글을 줄마다 나눈 질문들(빈 줄은 뺀다, 최대 `MAX_QUERIES`개).
pub fn question_lines(questions: &str) -> Vec<String> {
    questions.lines().map(str::trim).filter(|l| !l.is_empty()).take(MAX_QUERIES).map(String::from).collect()
}

/// 카드 한 장의 임베딩 글: `passage: ` + 제목 + 활동 요약(진행 순서) + 낱말.
pub fn card_passage(card: &Activity) -> String {
    let body = format!("{}\n{}\n{}", card.title, card.steps.join(" "), card.keywords.join(", "));
    format!("{}{}", embed::PASSAGE_PREFIX, embed::clip(&body, embed::MAX_PASSAGE_CHARS))
}

/// 카드가 지워지면 벡터도 지운다.
pub fn forget_card(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM embeddings WHERE kind = ?1 AND ref_id = ?2", params![KIND_CARD, id]).db()?;
    Ok(())
}

pub struct Missing {
    id: String,
    hash: String,
    text: String,
}

/// 서버에 물어볼 것: 질문들과, 저장된 벡터가 없거나 글이 바뀐 카드들.
pub struct Plan {
    queries: Vec<String>,
    missing: Vec<Missing>,
    cached: HashMap<String, Vec<f32>>,
}

pub struct Computed {
    query_vecs: Vec<Vec<f32>>,
    card_vecs: Vec<Vec<f32>>,
}

/// 뜻 점수를 매길 재료. `bonuses`가 카드마다 점수와 이유를 돌려준다.
pub struct Semantic {
    queries: Vec<(String, Vec<f32>)>,
    cards: HashMap<String, Vec<f32>>,
}

pub struct Meaning {
    pub points: i64,
    pub reason: String,
}

/// 질문이 없거나 카드가 없으면 `None`(규칙 점수만 쓴다).
pub fn plan(conn: &Connection, cards: &[Activity], questions: &str) -> DbResult<Option<Plan>> {
    let queries = question_lines(questions);
    if queries.is_empty() || cards.is_empty() {
        return Ok(None);
    }
    let mut stored: HashMap<String, (String, Vec<u8>)> = HashMap::new();
    let mut stmt = conn.prepare("SELECT ref_id, text_hash, vec FROM embeddings WHERE kind = ?1 AND model = ?2").db()?;
    let rows = stmt.query_map(params![KIND_CARD, embed::MODEL_ID], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?))).db()?;
    for row in rows {
        let (id, hash, blob) = row.db()?;
        stored.insert(id, (hash, blob));
    }
    let mut missing = Vec::new();
    let mut cached = HashMap::new();
    for card in cards {
        let text = card_passage(card);
        let hash = embed::text_hash(&text);
        let hit = stored.get(&card.id).filter(|(h, _)| *h == hash).and_then(|(_, blob)| embed::from_blob(blob));
        if let Some(v) = hit {
            cached.insert(card.id.clone(), v);
            continue;
        }
        missing.push(Missing { id: card.id.clone(), hash, text });
    }
    Ok(Some(Plan { queries, missing, cached }))
}

impl Plan {
    /// 서버로 새 카드 글과 질문을 한꺼번에 임베딩한다. 하나라도 실패하거나 모양이 다르면 `None`.
    pub fn compute(&self, embed_fn: EmbedFn<'_>) -> Option<Computed> {
        let mut texts: Vec<String> = self.missing.iter().map(|m| m.text.clone()).collect();
        texts.extend(self.queries.iter().map(|q| format!("{}{}", embed::QUERY_PREFIX, embed::clip(q, embed::MAX_QUERY_CHARS))));
        let mut vecs = match embed_fn(&texts) {
            Ok(v) => v,
            Err(e) => {
                crate::logging::error("embed", &format!("임베딩 실패, 규칙 추천으로 대신함: {}", crate::engine::tail(&e, 300)));
                return None;
            }
        };
        if vecs.len() != texts.len() || vecs.iter().any(|v| v.len() != embed::DIM) {
            crate::logging::error("embed", "임베딩 결과의 개수나 차원이 맞지 않아 규칙 추천으로 대신함");
            return None;
        }
        for v in vecs.iter_mut() {
            embed::normalize(v);
        }
        let query_vecs = vecs.split_off(self.missing.len());
        Some(Computed { query_vecs, card_vecs: vecs })
    }
}

/// 새로 계산한 카드 벡터를 저장하고 뜻 점수 재료를 만든다.
pub fn store_and_build(conn: &Connection, plan: Plan, computed: Computed) -> DbResult<Semantic> {
    let Plan { queries, missing, mut cached } = plan;
    let tx = conn.unchecked_transaction().db()?;
    for (m, v) in missing.iter().zip(&computed.card_vecs) {
        tx.execute(
            "INSERT OR REPLACE INTO embeddings (kind, ref_id, model, text_hash, vec) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![KIND_CARD, m.id, embed::MODEL_ID, m.hash, embed::to_blob(v)],
        )
        .db()?;
        cached.insert(m.id.clone(), v.clone());
    }
    tx.commit().db()?;
    Ok(Semantic { queries: queries.into_iter().zip(computed.query_vecs).collect(), cards: cached })
}

fn quote(question: &str) -> String {
    let n = question.chars().count();
    if n <= REASON_QUESTION_CHARS {
        question.to_string()
    } else {
        format!("{}…", question.chars().take(REASON_QUESTION_CHARS).collect::<String>())
    }
}

impl Semantic {
    /// 카드마다 질문들과의 코사인 최댓값 `s`를 구하고, 같은 주제책 카드끼리 `s` 순위 상위 3장에 +4, +3, +2를 준다.
    /// 같은 입력이면 같은 결과다(동점이면 카드 id 순).
    pub fn bonuses(&self, cards: &[Activity]) -> HashMap<String, Meaning> {
        let mut by_topic: BTreeMap<&str, Vec<(f32, &str, usize)>> = BTreeMap::new();
        for card in cards {
            let Some(cv) = self.cards.get(&card.id) else { continue };
            let best = self
                .queries
                .iter()
                .enumerate()
                .map(|(i, (_, qv))| (embed::cosine(qv, cv), i))
                .fold(None::<(f32, usize)>, |acc, x| match acc {
                    Some(a) if a.0 >= x.0 => Some(a),
                    _ => Some(x),
                });
            if let Some((s, qi)) = best {
                by_topic.entry(card.topic_book_id.as_str()).or_default().push((s, card.id.as_str(), qi));
            }
        }
        let mut out = HashMap::new();
        for (_, mut list) in by_topic {
            list.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
            for (&(_, id, qi), points) in list.iter().zip(RANK_POINTS) {
                out.insert(id.to_string(), Meaning { points, reason: format!("학생 질문 '{}'과 뜻이 가까워요", quote(&self.queries[qi].0)) });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, topic: &str, title: &str) -> Activity {
        Activity {
            id: id.into(),
            topic_book_id: topic.into(),
            title: title.into(),
            subject: "슬".into(),
            standards: vec!["[2슬02-04]".into()],
            kind: "general".into(),
            minutes: 40,
            steps: vec!["처음 모여서 이야기를 나눈다".into(), "직접 해 보며 살펴본다".into(), "알게 된 것을 정리한다".into()],
            materials: vec![],
            keywords: vec!["하나".into(), "둘".into(), "셋".into()],
            makingbook: None,
        }
    }

    fn unit_vec(axis: usize) -> Vec<f32> {
        let mut v = vec![0.0; embed::DIM];
        v[axis] = 1.0;
        v
    }

    /// 두 축을 섞은 단위 벡터(코사인을 정확히 정하려고).
    fn mix(a: usize, b: usize, wa: f32, wb: f32) -> Vec<f32> {
        let mut v = vec![0.0; embed::DIM];
        v[a] = wa;
        v[b] = wb;
        embed::normalize(&mut v);
        v
    }

    fn semantic(queries: Vec<(&str, Vec<f32>)>, cards: Vec<(&str, Vec<f32>)>) -> Semantic {
        Semantic { queries: queries.into_iter().map(|(q, v)| (q.to_string(), v)).collect(), cards: cards.into_iter().map(|(id, v)| (id.to_string(), v)).collect() }
    }

    #[test]
    fn question_lines_trim_skip_blank_and_cap() {
        assert_eq!(question_lines("  개미는 왜 줄 서요? \n\n  \n민들레는 어디서 와요?\r\n"), vec!["개미는 왜 줄 서요?", "민들레는 어디서 와요?"]);
        assert!(question_lines(" \n ").is_empty());
        let many = (0..50).map(|i| format!("질문 {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(question_lines(&many).len(), MAX_QUERIES);
    }

    #[test]
    fn card_passage_has_prefix_title_steps_and_keywords_within_the_limit() {
        let c = card("a", "t", "곤충 관찰하기");
        let p = card_passage(&c);
        assert!(p.starts_with("passage: 곤충 관찰하기\n"), "{p}");
        assert!(p.contains("처음 모여서 이야기를 나눈다 직접 해 보며"));
        assert!(p.ends_with("하나, 둘, 셋"));
        let mut long = c.clone();
        long.steps = vec!["가".repeat(60); 5];
        assert!(card_passage(&long).chars().count() <= embed::PASSAGE_PREFIX.chars().count() + embed::MAX_PASSAGE_CHARS);
    }

    #[test]
    fn ranks_top_three_per_topic_book_with_four_three_two_points() {
        let q = unit_vec(0);
        // 같은 주제책 t1: 코사인 1.0 > 0.8 > 0.6 > 0.0. 다른 주제책 t2의 한 장도 자기 책에서 1등이다.
        let cards = [card("a1", "t1", "카드 하나 하나"), card("a2", "t1", "카드 둘 둘둘"), card("a3", "t1", "카드 셋 셋셋"), card("a4", "t1", "카드 넷 넷넷"), card("b1", "t2", "다른 책 카드")];
        let s = semantic(
            vec![("개미는 왜 줄 서요?", q.clone())],
            vec![("a1", q.clone()), ("a2", mix(0, 1, 0.8, 0.6)), ("a3", mix(0, 1, 0.6, 0.8)), ("a4", unit_vec(2)), ("b1", unit_vec(3))],
        );
        let b = s.bonuses(&cards);
        let pts = |id: &str| b.get(id).map(|m| m.points);
        assert_eq!((pts("a1"), pts("a2"), pts("a3"), pts("a4")), (Some(4), Some(3), Some(2), None));
        assert_eq!(pts("b1"), Some(4), "절댓값이 낮아도 자기 주제책에서 1등이면 가점");
        assert_eq!(b["a1"].reason, "학생 질문 '개미는 왜 줄 서요?'과 뜻이 가까워요");
    }

    #[test]
    fn uses_the_best_question_for_each_card_and_clips_long_questions_in_the_reason() {
        let long = "개미는 왜 한 줄로 줄을 서서 가는 걸까요 정말 궁금해요";
        let s = semantic(vec![("무관한 질문", unit_vec(5)), (long, unit_vec(0))], vec![("a", unit_vec(0))]);
        let b = s.bonuses(&[card("a", "t", "곤충 관찰하기")]);
        let expected: String = long.chars().take(20).collect();
        assert_eq!(b["a"].reason, format!("학생 질문 '{expected}…'과 뜻이 가까워요"));
    }

    #[test]
    fn ties_break_by_card_id_and_result_is_deterministic() {
        let q = unit_vec(0);
        let cards = [card("c", "t", "카드 하나 하나"), card("a", "t", "카드 둘 둘둘"), card("b", "t", "카드 셋 셋셋"), card("d", "t", "카드 넷 넷넷")];
        let s = semantic(vec![("질문", q.clone())], cards.iter().map(|c| (c.id.as_str(), q.clone())).collect());
        let first = s.bonuses(&cards);
        let points = |m: &HashMap<String, Meaning>| {
            let mut v: Vec<(String, i64)> = m.iter().map(|(k, x)| (k.clone(), x.points)).collect();
            v.sort();
            v
        };
        assert_eq!(points(&first), vec![("a".into(), 4), ("b".into(), 3), ("c".into(), 2)]);
        assert_eq!(points(&first), points(&s.bonuses(&cards)));
    }

    #[test]
    fn cards_without_a_vector_get_no_bonus() {
        let s = semantic(vec![("질문", unit_vec(0))], vec![]);
        assert!(s.bonuses(&[card("a", "t", "카드 하나 하나")]).is_empty());
    }

    fn counting_embedder(calls: &std::cell::RefCell<Vec<Vec<String>>>) -> impl Fn(&[String]) -> Result<Vec<Vec<f32>>, String> + '_ {
        move |texts: &[String]| {
            calls.borrow_mut().push(texts.to_vec());
            Ok(texts.iter().map(|t| unit_vec(t.chars().count() % embed::DIM)).collect())
        }
    }

    #[test]
    fn card_vectors_are_stored_and_recomputed_only_when_the_text_hash_changes() {
        let conn = crate::db::test_conn();
        let mut a = card("a", "t", "곤충 관찰하기");
        let b = card("b", "t", "씨앗 심기 활동");
        let calls = std::cell::RefCell::new(Vec::new());
        let embedder = counting_embedder(&calls);
        let run = |cards: &[Activity]| {
            let plan = plan(&conn, cards, "개미는 왜 줄 서요?").unwrap().unwrap();
            let computed = plan.compute(&embedder).unwrap();
            store_and_build(&conn, plan, computed).unwrap();
        };

        run(&[a.clone(), b.clone()]);
        let first = calls.borrow().clone();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].len(), 3, "카드 2장 + 질문 1개");
        assert!(first[0][0].starts_with("passage: ") && first[0][1].starts_with("passage: "));
        assert_eq!(first[0][2], "query: 개미는 왜 줄 서요?");
        let n: i64 = conn.query_row("SELECT count(*) FROM embeddings WHERE model = 'e5-small-q8' AND kind = 'card'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let blob_len: i64 = conn.query_row("SELECT length(vec) FROM embeddings WHERE ref_id = 'a'", [], |r| r.get(0)).unwrap();
        assert_eq!(blob_len as usize, embed::DIM * 4);

        // 그대로면 질문만 다시 계산한다(카드는 저장된 벡터를 쓴다).
        run(&[a.clone(), b.clone()]);
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(calls.borrow()[1], vec!["query: 개미는 왜 줄 서요?".to_string()]);

        // 카드 글이 바뀌면 그 카드만 다시 계산한다.
        a.title = "개미 집 탐험하기".into();
        run(&[a.clone(), b.clone()]);
        let third = calls.borrow()[2].clone();
        assert_eq!(third.len(), 2);
        assert!(third[0].contains("개미 집 탐험하기"));
        let hash: String = conn.query_row("SELECT text_hash FROM embeddings WHERE ref_id = 'a'", [], |r| r.get(0)).unwrap();
        assert_eq!(hash, embed::text_hash(&card_passage(&a)));
        let n: i64 = conn.query_row("SELECT count(*) FROM embeddings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "같은 카드는 한 줄만 둔다");

        // 카드를 지우면 벡터도 지운다.
        forget_card(&conn, "a").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM embeddings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn question_vectors_are_never_stored() {
        let conn = crate::db::test_conn();
        let cards = [card("a", "t", "곤충 관찰하기")];
        let calls = std::cell::RefCell::new(Vec::new());
        let embedder = counting_embedder(&calls);
        let plan = plan(&conn, &cards, "비밀 질문입니다").unwrap().unwrap();
        let computed = plan.compute(&embedder).unwrap();
        store_and_build(&conn, plan, computed).unwrap();
        let kinds: Vec<String> = conn.prepare("SELECT DISTINCT kind FROM embeddings").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(kinds, vec!["card".to_string()]);
        let hits: i64 = conn.query_row("SELECT count(*) FROM embeddings WHERE ref_id LIKE '%비밀%' OR text_hash LIKE '%비밀%'", [], |r| r.get(0)).unwrap();
        assert_eq!(hits, 0);
    }

    #[test]
    fn no_questions_or_no_cards_means_no_plan_and_failures_mean_no_semantics() {
        let conn = crate::db::test_conn();
        let cards = [card("a", "t", "곤충 관찰하기")];
        assert!(plan(&conn, &cards, "  \n ").unwrap().is_none());
        assert!(plan(&conn, &[], "질문").unwrap().is_none());
        let p = plan(&conn, &cards, "질문").unwrap().unwrap();
        assert!(p.compute(&|_: &[String]| Err("서버 오류".to_string())).is_none());
        assert!(p.compute(&|t: &[String]| Ok(vec![unit_vec(0); t.len() - 1])).is_none(), "개수가 다르면 쓰지 않는다");
        assert!(p.compute(&|t: &[String]| Ok(vec![vec![1.0; 10]; t.len()])).is_none(), "차원이 다르면 쓰지 않는다");
        let n: i64 = conn.query_row("SELECT count(*) FROM embeddings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "실패하면 아무것도 저장하지 않는다");
    }

    #[test]
    fn a_corrupt_stored_vector_is_recomputed() {
        let conn = crate::db::test_conn();
        let cards = [card("a", "t", "곤충 관찰하기")];
        let hash = embed::text_hash(&card_passage(&cards[0]));
        conn.execute("INSERT INTO embeddings (kind, ref_id, model, text_hash, vec) VALUES ('card', 'a', ?1, ?2, x'0102')", params![embed::MODEL_ID, hash]).unwrap();
        let p = plan(&conn, &cards, "질문").unwrap().unwrap();
        assert_eq!(p.missing.len(), 1);
    }
}
