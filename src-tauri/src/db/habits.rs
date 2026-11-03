//! 습관 신호등: 학급 습관, 학생별 날짜 체크, 주간 표, 포트폴리오용 실천율.
//!
//! 날짜는 `YYYY-MM-DD` 문자열. "오늘"은 인자로 받아 테스트할 수 있게 한다.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use chrono::{Duration, NaiveDate};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{classes, lock, new_id, now_iso, students, DbExt, DbResult};

const MAX_TITLE_CHARS: usize = 30;
const MAX_ICON_CHARS: usize = 8;
/// 오늘부터 며칠 전까지 고칠 수 있는지.
const EDIT_DAYS_BACK: i64 = 7;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    pub id: String,
    pub class_id: String,
    pub title: String,
    pub icon: String,
    pub starts_on: Option<String>,
    pub ends_on: Option<String>,
    pub active: bool,
    pub position: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HabitInput {
    pub id: Option<String>,
    pub class_id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    pub starts_on: Option<String>,
    pub ends_on: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayEntry {
    pub student_id: String,
    pub number: i64,
    pub name: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HabitDay {
    pub habit: Habit,
    pub date: String,
    pub entries: Vec<DayEntry>,
    pub done_count: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToggleResult {
    pub done: bool,
    pub done_count: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeekRow {
    pub student_id: String,
    pub number: i64,
    pub name: String,
    /// 그날 이 습관에 체크가 하나도 없었으면 `None`.
    pub marks: Vec<Option<bool>>,
    pub done_days: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HabitWeek {
    pub dates: Vec<String>,
    pub rows: Vec<WeekRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HabitSummary {
    pub habit_id: String,
    pub title: String,
    pub icon: String,
    pub done_days: i64,
    pub school_days: i64,
}

fn parse_date(s: &str) -> DbResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "날짜 형식이 올바르지 않습니다".to_string())
}

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Habit> {
    Ok(Habit {
        id: r.get(0)?,
        class_id: r.get(1)?,
        title: r.get(2)?,
        icon: r.get(3)?,
        starts_on: r.get(4)?,
        ends_on: r.get(5)?,
        active: r.get::<_, i64>(6)? != 0,
        position: r.get(7)?,
        created_at: r.get(8)?,
    })
}

const COLS: &str = "id, class_id, title, icon, starts_on, ends_on, active, position, created_at";

pub fn get(conn: &Connection, id: &str) -> DbResult<Habit> {
    conn.query_row(&format!("SELECT {COLS} FROM habits WHERE id = ?1"), [id], row).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "습관을 찾을 수 없습니다".to_string(),
        e => format!("데이터베이스 오류: {e}"),
    })
}

pub fn list(conn: &Connection, class_id: &str, include_inactive: bool) -> DbResult<Vec<Habit>> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLS} FROM habits WHERE class_id = ?1 AND (?2 OR active = 1) ORDER BY position, created_at, id"))
        .db()?;
    let rows = stmt.query_map(params![class_id, include_inactive], row).db()?;
    rows.collect::<Result<_, _>>().db()
}

fn clean_date(d: &Option<String>) -> DbResult<Option<NaiveDate>> {
    match d.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => parse_date(s).map(Some),
    }
}

pub fn save(conn: &Connection, input: &HabitInput) -> DbResult<Habit> {
    classes::get(conn, &input.class_id)?;
    let title = input.title.trim();
    if title.is_empty() {
        return Err("습관 이름을 입력해 주세요".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err("습관 이름이 너무 깁니다".to_string());
    }
    let icon = input.icon.trim();
    if icon.chars().count() > MAX_ICON_CHARS {
        return Err("아이콘은 이모지 한 개만 넣어 주세요".to_string());
    }
    let starts = clean_date(&input.starts_on)?;
    let ends = clean_date(&input.ends_on)?;
    if let (Some(s), Some(e)) = (starts, ends) {
        if e < s {
            return Err("끝나는 날이 시작하는 날보다 빠릅니다".to_string());
        }
    }
    let (starts, ends) = (starts.map(fmt), ends.map(fmt));
    match input.id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => {
            let cur = get(conn, id)?;
            if cur.class_id != input.class_id {
                return Err("다른 학급의 습관입니다".to_string());
            }
            conn.execute(
                "UPDATE habits SET title = ?2, icon = ?3, starts_on = ?4, ends_on = ?5, active = ?6 WHERE id = ?1",
                params![id, title, icon, starts, ends, input.active],
            )
            .db()?;
            get(conn, id)
        }
        None => {
            let id = new_id();
            let position: i64 =
                conn.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM habits WHERE class_id = ?1", [&input.class_id], |r| r.get(0)).db()?;
            conn.execute(
                "INSERT INTO habits (id, class_id, title, icon, starts_on, ends_on, active, position, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, input.class_id, title, icon, starts, ends, input.active, position, now_iso()],
            )
            .db()?;
            get(conn, &id)
        }
    }
}

pub fn delete(conn: &Connection, id: &str) -> DbResult<()> {
    let n = conn.execute("DELETE FROM habits WHERE id = ?1", [id]).db()?;
    if n == 0 {
        return Err("습관을 찾을 수 없습니다".to_string());
    }
    Ok(())
}

fn active_students(conn: &Connection, class_id: &str) -> DbResult<Vec<students::Student>> {
    students::list(conn, class_id, false)
}

fn day_counts(conn: &Connection, habit: &Habit, date: &str) -> DbResult<(HashMap<String, bool>, i64, i64)> {
    let stu = active_students(conn, &habit.class_id)?;
    let ids: HashSet<&str> = stu.iter().map(|s| s.id.as_str()).collect();
    let mut stmt = conn.prepare("SELECT student_id, done FROM habit_checks WHERE habit_id = ?1 AND checked_on = ?2").db()?;
    let checks: Vec<(String, bool)> =
        stmt.query_map(params![habit.id, date], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0))).db()?.collect::<Result<_, _>>().db()?;
    let map: HashMap<String, bool> = checks.into_iter().filter(|(id, _)| ids.contains(id.as_str())).collect();
    let done = map.values().filter(|d| **d).count() as i64;
    Ok((map, done, stu.len() as i64))
}

pub fn day(conn: &Connection, habit_id: &str, date: &str) -> DbResult<HabitDay> {
    let date = fmt(parse_date(date)?);
    let habit = get(conn, habit_id)?;
    let stu = active_students(conn, &habit.class_id)?;
    let (map, done_count, total) = day_counts(conn, &habit, &date)?;
    let entries = stu
        .into_iter()
        .map(|s| DayEntry { done: map.get(&s.id).copied().unwrap_or(false), student_id: s.id, number: s.number, name: s.name })
        .collect();
    Ok(HabitDay { habit, date, entries, done_count, total })
}

pub fn toggle(conn: &Connection, habit_id: &str, student_id: &str, date: &str, today: NaiveDate) -> DbResult<ToggleResult> {
    let d = parse_date(date)?;
    if d > today {
        return Err("오늘보다 앞날은 체크할 수 없습니다".to_string());
    }
    if d < today - Duration::days(EDIT_DAYS_BACK) {
        return Err(format!("{EDIT_DAYS_BACK}일 전까지만 고칠 수 있습니다"));
    }
    let habit = get(conn, habit_id)?;
    let student = students::get(conn, student_id)?;
    if student.class_id != habit.class_id {
        return Err("이 습관의 학급 학생이 아닙니다".to_string());
    }
    if !student.active {
        return Err("전학·퇴원한 학생은 체크할 수 없습니다".to_string());
    }
    let date = fmt(d);
    let cur: Option<i64> = conn
        .query_row(
            "SELECT done FROM habit_checks WHERE habit_id = ?1 AND student_id = ?2 AND checked_on = ?3",
            params![habit_id, student_id, date],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        .db()?;
    let done = cur.unwrap_or(0) == 0;
    conn.execute(
        "INSERT INTO habit_checks (habit_id, student_id, checked_on, done, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(habit_id, student_id, checked_on) DO UPDATE SET done = excluded.done, updated_at = excluded.updated_at",
        params![habit_id, student_id, date, done, now_iso()],
    )
    .db()?;
    let (_, done_count, total) = day_counts(conn, &habit, &date)?;
    Ok(ToggleResult { done, done_count, total })
}

pub fn week(conn: &Connection, habit_id: &str, end_date: &str) -> DbResult<HabitWeek> {
    let end = parse_date(end_date)?;
    let habit = get(conn, habit_id)?;
    let dates: Vec<String> = (0..7).rev().map(|i| fmt(end - Duration::days(i))).collect();
    let stu = active_students(conn, &habit.class_id)?;
    let mut stmt = conn.prepare("SELECT student_id, checked_on, done FROM habit_checks WHERE habit_id = ?1 AND checked_on BETWEEN ?2 AND ?3").db()?;
    let checks: Vec<(String, String, bool)> = stmt
        .query_map(params![habit_id, dates[0], dates[6]], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))
        .db()?
        .collect::<Result<_, _>>()
        .db()?;
    let by_cell: HashMap<(&str, &str), bool> = checks.iter().map(|(s, d, v)| ((s.as_str(), d.as_str()), *v)).collect();
    let days_with_checks: HashSet<&str> = checks.iter().map(|(_, d, _)| d.as_str()).collect();
    let rows = stu
        .into_iter()
        .map(|s| {
            let marks: Vec<Option<bool>> = dates
                .iter()
                .map(|d| {
                    if !days_with_checks.contains(d.as_str()) {
                        None
                    } else {
                        Some(by_cell.get(&(s.id.as_str(), d.as_str())).copied().unwrap_or(false))
                    }
                })
                .collect();
            let done_days = marks.iter().filter(|m| **m == Some(true)).count() as i64;
            WeekRow { student_id: s.id, number: s.number, name: s.name, marks, done_days }
        })
        .collect();
    Ok(HabitWeek { dates, rows })
}

/// 최근 `days`일(끝날 포함) 동안 습관별 실천 일수와 분모(학교에 체크가 한 번이라도 있었던 날 수).
/// 습관 기간 밖의 날은 세지 않는다. 기록이 없는 비활성 습관은 뺀다.
pub fn student_summary(conn: &Connection, student_id: &str, end_date: &str, days: i64) -> DbResult<Vec<HabitSummary>> {
    if !(1..=366).contains(&days) {
        return Err("기간이 올바르지 않습니다".to_string());
    }
    let end = parse_date(end_date)?;
    let student = students::get(conn, student_id)?;
    let from = end - Duration::days(days - 1);
    let mut out = Vec::new();
    for h in list(conn, &student.class_id, true)? {
        let lo = h.starts_on.as_deref().map(parse_date).transpose()?.map_or(from, |s| s.max(from));
        let hi = h.ends_on.as_deref().map(parse_date).transpose()?.map_or(end, |e| e.min(end));
        if lo > hi {
            continue;
        }
        let (lo, hi) = (fmt(lo), fmt(hi));
        let school_days: i64 = conn
            .query_row(
                "SELECT count(DISTINCT checked_on) FROM habit_checks WHERE habit_id = ?1 AND checked_on BETWEEN ?2 AND ?3",
                params![h.id, lo, hi],
                |r| r.get(0),
            )
            .db()?;
        if !h.active && school_days == 0 {
            continue;
        }
        let done_days: i64 = conn
            .query_row(
                "SELECT count(*) FROM habit_checks WHERE habit_id = ?1 AND student_id = ?2 AND done = 1 AND checked_on BETWEEN ?3 AND ?4",
                params![h.id, student_id, lo, hi],
                |r| r.get(0),
            )
            .db()?;
        out.push(HabitSummary { habit_id: h.id, title: h.title, icon: h.icon, done_days, school_days });
    }
    Ok(out)
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

#[tauri::command]
pub fn habit_list(db: State<Mutex<Connection>>, class_id: String, include_inactive: bool) -> DbResult<Vec<Habit>> {
    list(&*lock(&db)?, &class_id, include_inactive)
}

#[tauri::command]
pub fn habit_save(db: State<Mutex<Connection>>, input: HabitInput) -> DbResult<Habit> {
    save(&*lock(&db)?, &input)
}

#[tauri::command]
pub fn habit_delete(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    delete(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn habit_day(db: State<Mutex<Connection>>, habit_id: String, date: String) -> DbResult<HabitDay> {
    day(&*lock(&db)?, &habit_id, &date)
}

#[tauri::command]
pub fn habit_toggle(db: State<Mutex<Connection>>, habit_id: String, student_id: String, date: String) -> DbResult<ToggleResult> {
    toggle(&*lock(&db)?, &habit_id, &student_id, &date, today())
}

#[tauri::command]
pub fn habit_week(db: State<Mutex<Connection>>, habit_id: String, end_date: String) -> DbResult<HabitWeek> {
    week(&*lock(&db)?, &habit_id, &end_date)
}

#[tauri::command]
pub fn habit_student_summary(db: State<Mutex<Connection>>, student_id: String, end_date: String, days: i64) -> DbResult<Vec<HabitSummary>> {
    student_summary(&*lock(&db)?, &student_id, &end_date, days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    fn d(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    fn input(class: &str, title: &str) -> HabitInput {
        HabitInput { id: None, class_id: class.into(), title: title.into(), icon: "🧹".into(), starts_on: None, ends_on: None, active: true }
    }

    /// 학급 1(학생 3), 학급 2(학생 1), 습관 1개.
    fn seed() -> (Connection, String, Vec<String>, String, String) {
        let mut conn = test_conn();
        let c = classes::create(&conn, 2027, 1, "1반").unwrap();
        students::import(&mut conn, &c.id, "1 가\n2 나\n3 다").unwrap();
        let ids = students::list(&conn, &c.id, true).unwrap().into_iter().map(|s| s.id).collect();
        let c2 = classes::create(&conn, 2027, 1, "2반").unwrap();
        students::import(&mut conn, &c2.id, "1 라").unwrap();
        let other = students::list(&conn, &c2.id, true).unwrap().remove(0).id;
        let h = save(&conn, &input(&c.id, "사물함 정리하기")).unwrap();
        (conn, c.id, ids, other, h.id)
    }

    const TODAY: &str = "2027-03-10";

    #[test]
    fn save_update_deactivate_and_validate() {
        let (conn, cid, _, _, hid) = seed();
        let second = save(&conn, &input(&cid, "고운 말 쓰기")).unwrap();
        assert_eq!(second.position, 1);
        let mut edit = input(&cid, " 고운 말 쓰기2 ");
        edit.id = Some(hid.clone());
        edit.starts_on = Some("2027-03-02".into());
        edit.ends_on = Some("2027-03-31".into());
        edit.active = false;
        let h = save(&conn, &edit).unwrap();
        assert_eq!((h.title.as_str(), h.starts_on.as_deref(), h.active), ("고운 말 쓰기2", Some("2027-03-02"), false));
        assert_eq!(list(&conn, &cid, false).unwrap().len(), 1);
        assert_eq!(list(&conn, &cid, true).unwrap().len(), 2);
        assert!(save(&conn, &input(&cid, "  ")).is_err());
        let mut bad = input(&cid, "x");
        bad.starts_on = Some("2027-04-01".into());
        bad.ends_on = Some("2027-03-01".into());
        assert!(save(&conn, &bad).is_err());
        bad.starts_on = Some("어제".into());
        assert!(save(&conn, &bad).is_err());
        assert!(save(&conn, &input("none", "x")).is_err());
        delete(&conn, &second.id).unwrap();
        assert!(delete(&conn, &second.id).is_err());
    }

    #[test]
    fn toggle_twice_restores_and_day_counts() {
        let (conn, _, ids, _, hid) = seed();
        let t = d(TODAY);
        let r = toggle(&conn, &hid, &ids[0], TODAY, t).unwrap();
        assert_eq!((r.done, r.done_count, r.total), (true, 1, 3));
        toggle(&conn, &hid, &ids[1], TODAY, t).unwrap();
        let day1 = day(&conn, &hid, TODAY).unwrap();
        assert_eq!((day1.done_count, day1.total), (2, 3));
        assert_eq!(day1.entries.iter().map(|e| e.done).collect::<Vec<_>>(), [true, true, false]);
        let r = toggle(&conn, &hid, &ids[0], TODAY, t).unwrap();
        assert_eq!((r.done, r.done_count), (false, 1));
        let r = toggle(&conn, &hid, &ids[0], TODAY, t).unwrap();
        assert!(r.done);
        let r = toggle(&conn, &hid, &ids[0], TODAY, t).unwrap();
        assert!(!r.done);
        assert!(!day(&conn, &hid, TODAY).unwrap().entries[0].done);
    }

    #[test]
    fn toggle_rejects_other_class_future_and_old_dates() {
        let (conn, _, ids, other, hid) = seed();
        let t = d(TODAY);
        assert!(toggle(&conn, &hid, &other, TODAY, t).is_err());
        assert!(toggle(&conn, &hid, "none", TODAY, t).is_err());
        assert!(toggle(&conn, &hid, &ids[0], "2027-03-11", t).is_err());
        assert!(toggle(&conn, &hid, &ids[0], "2027-03-02", t).is_err()); // 8일 전
        assert!(toggle(&conn, &hid, &ids[0], "2027-03-03", t).is_ok()); // 7일 전
        assert!(toggle(&conn, &hid, &ids[0], "bad", t).is_err());
        assert!(toggle(&conn, "none", &ids[0], TODAY, t).is_err());
    }

    #[test]
    fn transferred_students_are_left_out_of_day() {
        let (conn, _, ids, _, hid) = seed();
        let t = d(TODAY);
        toggle(&conn, &hid, &ids[2], TODAY, t).unwrap();
        conn.execute("UPDATE students SET active = 0 WHERE id = ?1", [&ids[2]]).unwrap();
        let day1 = day(&conn, &hid, TODAY).unwrap();
        assert_eq!((day1.done_count, day1.total, day1.entries.len()), (0, 2, 2));
        assert!(toggle(&conn, &hid, &ids[2], TODAY, t).is_err());
    }

    #[test]
    fn week_has_seven_cells_with_null_for_days_without_checks() {
        let (conn, _, ids, _, hid) = seed();
        let t = d(TODAY);
        toggle(&conn, &hid, &ids[0], "2027-03-10", t).unwrap();
        toggle(&conn, &hid, &ids[0], "2027-03-08", t).unwrap();
        toggle(&conn, &hid, &ids[1], "2027-03-08", t).unwrap();
        toggle(&conn, &hid, &ids[1], "2027-03-08", t).unwrap(); // 취소: 체크는 있었음(아직)
        let w = week(&conn, &hid, TODAY).unwrap();
        assert_eq!(w.dates.len(), 7);
        assert_eq!((w.dates[0].as_str(), w.dates[6].as_str()), ("2027-03-04", "2027-03-10"));
        assert_eq!(w.rows.len(), 3);
        for r in &w.rows {
            assert_eq!(r.marks.len(), 7);
        }
        // 3/8 = index 4, 3/10 = index 6, 나머지는 체크 없음.
        assert_eq!(w.rows[0].marks, [None, None, None, None, Some(true), None, Some(true)]);
        assert_eq!(w.rows[0].done_days, 2);
        assert_eq!(w.rows[1].marks[4], Some(false));
        assert_eq!(w.rows[1].marks[6], Some(false));
        assert_eq!(w.rows[1].done_days, 0);
        assert_eq!(w.rows[2].marks[6], Some(false));
    }

    #[test]
    fn student_summary_uses_school_days_as_denominator() {
        let (conn, cid, ids, other, hid) = seed();
        let t = d(TODAY);
        // 3/3(7일 전), 3/8, 3/10에 누군가 체크 → 학교 날짜 3일. 학생1은 3/3·3/10 실천.
        toggle(&conn, &hid, &ids[0], "2027-03-03", t).unwrap();
        toggle(&conn, &hid, &ids[1], "2027-03-08", t).unwrap();
        toggle(&conn, &hid, &ids[0], "2027-03-10", t).unwrap();
        let s = student_summary(&conn, &ids[0], TODAY, 28).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].done_days, s[0].school_days), (2, 3));
        let s = student_summary(&conn, &ids[2], TODAY, 28).unwrap();
        assert_eq!((s[0].done_days, s[0].school_days), (0, 3));
        // 기간을 7일로 줄이면 3/3(7일 전)은 윈도우(3/4~3/10) 밖.
        let s = student_summary(&conn, &ids[0], TODAY, 7).unwrap();
        assert_eq!((s[0].done_days, s[0].school_days), (1, 2));
        // 습관 기간(3/9~)이면 3/3·3/8은 빠진다.
        let mut edit = input(&cid, "사물함 정리하기");
        edit.id = Some(hid.clone());
        edit.starts_on = Some("2027-03-09".into());
        save(&conn, &edit).unwrap();
        let s = student_summary(&conn, &ids[0], TODAY, 28).unwrap();
        assert_eq!((s[0].done_days, s[0].school_days), (1, 1));
        // 다른 학급 학생에게는 이 습관이 없다.
        assert!(student_summary(&conn, &other, TODAY, 28).unwrap().is_empty());
        // 비활성이고 기록 없는 습관은 숨긴다.
        let mut off = input(&cid, "쉬는 습관");
        off.active = false;
        save(&conn, &off).unwrap();
        assert_eq!(student_summary(&conn, &ids[0], TODAY, 28).unwrap().len(), 1);
        assert!(student_summary(&conn, &ids[0], TODAY, 0).is_err());
    }
}
