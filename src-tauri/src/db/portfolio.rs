//! 학생별 포트폴리오와 학급 현황. 삭제된 관찰은 세지 않는다.

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use super::students::{self, Student};
use super::{lock, DbExt, DbResult};

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopicCount {
    pub topic_book_id: String,
    pub count: i64,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StandardCount {
    pub standard_code: String,
    pub count: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudentPortfolio {
    pub student: Student,
    pub observation_count: i64,
    pub by_topic_book: Vec<TopicCount>,
    pub by_standard: Vec<StandardCount>,
    pub by_tag: Vec<TagCount>,
    pub first_observed_at: Option<String>,
    pub last_observed_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassRow {
    pub student: Student,
    pub total: i64,
    pub by_topic_book: HashMap<String, i64>,
    pub last_observed_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClassOverview {
    pub students: Vec<ClassRow>,
}

/// (키, 개수) 목록을 개수 내림차순, 키 오름차순으로.
fn counts(conn: &Connection, sql: &str, student_id: &str) -> DbResult<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(sql).db()?;
    let rows = stmt.query_map([student_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).db()?;
    rows.collect::<Result<_, _>>().db()
}

pub fn student(conn: &Connection, student_id: &str) -> DbResult<StudentPortfolio> {
    let student = students::get(conn, student_id)?;
    let (observation_count, first, last): (i64, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT count(*), MIN(o.observed_at), MAX(o.observed_at) FROM observation_students os
             JOIN observations o ON o.id = os.observation_id
             WHERE os.student_id = ?1 AND o.deleted_at IS NULL",
            [student_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .db()?;
    let by_topic_book = counts(
        conn,
        "SELECT o.topic_book_id, count(*) AS n FROM observation_students os
         JOIN observations o ON o.id = os.observation_id
         WHERE os.student_id = ?1 AND o.deleted_at IS NULL AND o.topic_book_id IS NOT NULL
         GROUP BY o.topic_book_id ORDER BY n DESC, o.topic_book_id",
        student_id,
    )?
    .into_iter()
    .map(|(topic_book_id, count)| TopicCount { topic_book_id, count })
    .collect();
    let by_standard = counts(
        conn,
        "SELECT st.standard_code, count(*) AS n FROM observation_students os
         JOIN observations o ON o.id = os.observation_id
         JOIN observation_standards st ON st.observation_id = o.id
         WHERE os.student_id = ?1 AND o.deleted_at IS NULL
         GROUP BY st.standard_code ORDER BY n DESC, st.standard_code",
        student_id,
    )?
    .into_iter()
    .map(|(standard_code, count)| StandardCount { standard_code, count })
    .collect();
    let by_tag = counts(
        conn,
        "SELECT t.tag, count(*) AS n FROM observation_students os
         JOIN observations o ON o.id = os.observation_id
         JOIN observation_tags t ON t.observation_id = o.id
         WHERE os.student_id = ?1 AND o.deleted_at IS NULL
         GROUP BY t.tag ORDER BY n DESC, t.tag",
        student_id,
    )?
    .into_iter()
    .map(|(tag, count)| TagCount { tag, count })
    .collect();
    Ok(StudentPortfolio { student, observation_count, by_topic_book, by_standard, by_tag, first_observed_at: first, last_observed_at: last })
}

pub fn class(conn: &Connection, class_id: &str) -> DbResult<ClassOverview> {
    let list = students::list(conn, class_id, false)?;
    let mut rows = Vec::with_capacity(list.len());
    for s in list {
        let p = student(conn, &s.id)?;
        rows.push(ClassRow {
            total: p.observation_count,
            by_topic_book: p.by_topic_book.into_iter().map(|t| (t.topic_book_id, t.count)).collect(),
            last_observed_at: p.last_observed_at,
            student: s,
        });
    }
    Ok(ClassOverview { students: rows })
}

#[tauri::command]
pub fn portfolio_student(db: State<Mutex<Connection>>, student_id: String) -> DbResult<StudentPortfolio> {
    student(&*lock(&db)?, &student_id)
}

#[tauri::command]
pub fn portfolio_class(db: State<Mutex<Connection>>, class_id: String) -> DbResult<ClassOverview> {
    class(&*lock(&db)?, &class_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::observations::tests::{input, media, seed};
    use crate::db::observations::{self, delete};

    #[test]
    fn student_counts_and_exclude_deleted() {
        let (mut conn, cid, ids, oid) = seed();
        // 학생1·2: 관찰 A(g1s1-school, [2바01-01], 협력). 학생1만 추가 B.
        let mut b = input(&cid, &[&ids[0]]);
        b.topic_book_id = Some("g1s1-people".into());
        b.standard_codes = vec!["[2바01-01]".into(), "[2슬01-02]".into()];
        b.tags = vec!["협력".into(), "배려".into()];
        b.observed_at = "2027-03-09T09:00:00+09:00".into();
        let b = observations::create(&mut conn, &media(), &b).unwrap();

        let p = student(&conn, &ids[0]).unwrap();
        assert_eq!(p.observation_count, 2);
        assert_eq!(p.first_observed_at.as_deref(), Some("2027-03-05T10:20:00+09:00"));
        assert_eq!(p.last_observed_at.as_deref(), Some("2027-03-09T09:00:00+09:00"));
        assert_eq!(p.by_topic_book.len(), 2);
        assert_eq!(p.by_standard[0], StandardCount { standard_code: "[2바01-01]".into(), count: 2 });
        assert_eq!(p.by_tag[0], TagCount { tag: "협력".into(), count: 2 });
        assert_eq!(student(&conn, &ids[1]).unwrap().observation_count, 1);
        assert_eq!(student(&conn, &ids[2]).unwrap().observation_count, 0);

        delete(&conn, &b.id).unwrap();
        let p = student(&conn, &ids[0]).unwrap();
        assert_eq!(p.observation_count, 1);
        assert_eq!(p.by_tag, [TagCount { tag: "협력".into(), count: 1 }]);
        assert_eq!(p.last_observed_at.as_deref(), Some("2027-03-05T10:20:00+09:00"));
        delete(&conn, &oid).unwrap();
        let p = student(&conn, &ids[0]).unwrap();
        assert_eq!((p.observation_count, p.first_observed_at), (0, None));
        assert!(p.by_standard.is_empty());
    }

    #[test]
    fn class_overview_counts_by_topic_book() {
        let (mut conn, cid, ids, oid) = seed();
        let mut c = input(&cid, &[&ids[0]]);
        c.topic_book_id = Some("g1s1-people".into());
        let c = observations::create(&mut conn, &media(), &c).unwrap();
        let mut none = input(&cid, &[&ids[0]]);
        none.topic_book_id = None;
        observations::create(&mut conn, &media(), &none).unwrap();

        let o = class(&conn, &cid).unwrap();
        assert_eq!(o.students.len(), 3);
        let r0 = &o.students[0];
        assert_eq!(r0.total, 3);
        assert_eq!(r0.by_topic_book["g1s1-school"], 1);
        assert_eq!(r0.by_topic_book["g1s1-people"], 1);
        assert_eq!(o.students[1].total, 1);
        assert_eq!(o.students[2].total, 0);
        assert!(o.students[2].last_observed_at.is_none());

        delete(&conn, &c.id).unwrap();
        delete(&conn, &oid).unwrap();
        let o = class(&conn, &cid).unwrap();
        assert_eq!(o.students[0].total, 1);
        assert!(!o.students[0].by_topic_book.contains_key("g1s1-people"));
        assert_eq!(o.students[1].total, 0);
    }
}
