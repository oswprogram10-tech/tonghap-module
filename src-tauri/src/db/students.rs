//! 학생 명단.

use std::sync::Mutex;

use regex::Regex;
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

use super::{lock, new_id, now_iso, DbExt, DbResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Student {
    pub id: String,
    pub class_id: String,
    pub number: i64,
    pub name: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub added: usize,
    pub updated: usize,
    pub skipped: Vec<String>,
}

const MAX_NUMBER: i64 = 200;
const MAX_NAME_CHARS: usize = 20;

fn row(r: &rusqlite::Row) -> rusqlite::Result<Student> {
    Ok(Student {
        id: r.get(0)?,
        class_id: r.get(1)?,
        number: r.get(2)?,
        name: r.get(3)?,
        active: r.get::<_, i64>(4)? != 0,
        created_at: r.get(5)?,
    })
}

const COLS: &str = "id, class_id, number, name, active, created_at";

pub fn get(conn: &Connection, id: &str) -> DbResult<Student> {
    conn.query_row(&format!("SELECT {COLS} FROM students WHERE id = ?1"), [id], row).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "학생을 찾을 수 없습니다".to_string(),
        e => format!("데이터베이스 오류: {e}"),
    })
}

pub fn list(conn: &Connection, class_id: &str, include_inactive: bool) -> DbResult<Vec<Student>> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLS} FROM students WHERE class_id = ?1 AND (?2 OR active = 1) ORDER BY number"))
        .db()?;
    let rows = stmt.query_map(params![class_id, include_inactive], row).db()?;
    rows.collect::<Result<_, _>>().db()
}

/// 한 줄을 (번호, 이름)으로 해석한다. 번호가 없으면 `None`. 해석할 수 없으면 `Err(())`.
fn parse_line(line: &str, re: &Regex) -> Result<(Option<i64>, String), ()> {
    if let Some(c) = re.captures(line) {
        let number: i64 = c[1].parse().map_err(|_| ())?;
        let name = c[2].trim().to_string();
        if !(1..=MAX_NUMBER).contains(&number) || name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(());
        }
        return Ok((Some(number), name));
    }
    // 이름만: 숫자로 시작하지 않아야 한다.
    if line.chars().next().is_some_and(|c| c.is_ascii_digit()) || line.chars().count() > MAX_NAME_CHARS {
        return Err(());
    }
    Ok((None, line.to_string()))
}

pub fn import(conn: &mut Connection, class_id: &str, text: &str) -> DbResult<ImportResult> {
    super::classes::get(conn, class_id)?;
    let re = Regex::new(r"^(\d{1,4})\s*[.\t ]\s*(.+)$").expect("정규식");
    let tx = conn.transaction().db()?;
    let mut result = ImportResult::default();
    let mut max: i64 = tx
        .query_row("SELECT COALESCE(MAX(number), 0) FROM students WHERE class_id = ?1", [class_id], |r| r.get(0))
        .db()?;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Ok((number, name)) = parse_line(line, &re) else {
            result.skipped.push(line.to_string());
            continue;
        };
        let number = match number {
            Some(n) => n,
            None => {
                if max + 1 > MAX_NUMBER {
                    result.skipped.push(line.to_string());
                    continue;
                }
                max + 1
            }
        };
        max = max.max(number);
        let existing: Option<(String, String)> = tx
            .query_row("SELECT id, name FROM students WHERE class_id = ?1 AND number = ?2", params![class_id, number], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
            .db()?;
        match existing {
            Some((id, old)) => {
                if old != name {
                    tx.execute("UPDATE students SET name = ?2 WHERE id = ?1", params![id, name]).db()?;
                    result.updated += 1;
                }
            }
            None => {
                tx.execute(
                    "INSERT INTO students (id, class_id, number, name, active, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                    params![new_id(), class_id, number, name, now_iso()],
                )
                .db()?;
                result.added += 1;
            }
        }
    }
    tx.commit().db()?;
    Ok(result)
}

pub fn update(conn: &Connection, id: &str, number: i64, name: &str, active: bool) -> DbResult<Student> {
    let name = name.trim();
    if name.is_empty() {
        return Err("학생 이름을 입력해 주세요".to_string());
    }
    if !(1..=MAX_NUMBER).contains(&number) {
        return Err(format!("번호는 1부터 {MAX_NUMBER} 사이여야 합니다"));
    }
    let current = get(conn, id)?;
    let clash: i64 = conn
        .query_row(
            "SELECT count(*) FROM students WHERE class_id = ?1 AND number = ?2 AND id <> ?3",
            params![current.class_id, number, id],
            |r| r.get(0),
        )
        .db()?;
    if clash > 0 {
        return Err(format!("{number}번 학생이 이미 있습니다"));
    }
    conn.execute("UPDATE students SET number = ?2, name = ?3, active = ?4 WHERE id = ?1", params![id, number, name, active]).db()?;
    get(conn, id)
}

#[tauri::command]
pub fn student_list(db: State<Mutex<Connection>>, class_id: String, include_inactive: bool) -> DbResult<Vec<Student>> {
    list(&*lock(&db)?, &class_id, include_inactive)
}

#[tauri::command]
pub fn student_import(db: State<Mutex<Connection>>, class_id: String, text: String) -> DbResult<ImportResult> {
    import(&mut *lock(&db)?, &class_id, &text)
}


#[tauri::command]
pub fn student_update(db: State<Mutex<Connection>>, id: String, number: i64, name: String, active: bool) -> DbResult<Student> {
    update(&*lock(&db)?, &id, number, &name, active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{classes, test_conn};

    fn setup() -> (Connection, String) {
        let conn = test_conn();
        let c = classes::create(&conn, 2027, 1, "1반").unwrap();
        (conn, c.id)
    }

    fn names(conn: &Connection, class: &str) -> Vec<(i64, String)> {
        list(conn, class, true).unwrap().into_iter().map(|s| (s.number, s.name)).collect()
    }

    #[test]
    fn import_four_line_formats() {
        let (mut conn, cid) = setup();
        let text = "1 김하늘\n2\t이바다\n3. 박구름\n4.최별\n최해\n\n";
        let r = import(&mut conn, &cid, text).unwrap();
        assert_eq!((r.added, r.updated), (5, 0));
        assert!(r.skipped.is_empty());
        assert_eq!(
            names(&conn, &cid),
            [(1, "김하늘".into()), (2, "이바다".into()), (3, "박구름".into()), (4, "최별".into()), (5, "최해".into())]
        );
    }

    #[test]
    fn name_only_continues_after_max() {
        let (mut conn, cid) = setup();
        import(&mut conn, &cid, "7 가나").unwrap();
        import(&mut conn, &cid, "다라\n마바").unwrap();
        let nums: Vec<_> = names(&conn, &cid).into_iter().map(|x| x.0).collect();
        assert_eq!(nums, [7, 8, 9]);
    }

    #[test]
    fn duplicate_number_updates_name() {
        let (mut conn, cid) = setup();
        import(&mut conn, &cid, "1 김하늘\n2 이바다").unwrap();
        let r = import(&mut conn, &cid, "1 김하나\n2 이바다\n3 새친구").unwrap();
        assert_eq!((r.added, r.updated), (1, 1));
        assert_eq!(names(&conn, &cid)[0], (1, "김하나".to_string()));
    }

    #[test]
    fn unparseable_lines_are_skipped() {
        let (mut conn, cid) = setup();
        let r = import(&mut conn, &cid, "12\n0 영번\n999 너무큼\n3명 이상\n정상이름\n5 가나").unwrap();
        assert_eq!(r.added, 2);
        assert_eq!(r.skipped, ["12", "0 영번", "999 너무큼", "3명 이상"]);
    }

    #[test]
    fn update_checks_number_clash_and_active() {
        let (mut conn, cid) = setup();
        import(&mut conn, &cid, "1 가\n2 나").unwrap();
        let s = list(&conn, &cid, false).unwrap();
        assert!(update(&conn, &s[0].id, 2, "가", true).is_err());
        let u = update(&conn, &s[0].id, 5, "가가", false).unwrap();
        assert!(!u.active);
        assert_eq!(list(&conn, &cid, false).unwrap().len(), 1);
        assert_eq!(list(&conn, &cid, true).unwrap().len(), 2);
    }
}
