//! 학급.

use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

use super::{lock, new_id, now_iso, DbExt, DbResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassInfo {
    pub id: String,
    pub school_year: i64,
    pub grade: i64,
    pub name: String,
    pub created_at: String,
}

fn validate(grade: i64, name: &str) -> DbResult<String> {
    if grade != 1 && grade != 2 {
        return Err("학년은 1 또는 2여야 합니다".to_string());
    }
    let name = name.trim();
    if name.is_empty() {
        return Err("학급 이름을 입력해 주세요".to_string());
    }
    Ok(name.to_string())
}

pub fn get(conn: &Connection, id: &str) -> DbResult<ClassInfo> {
    conn.query_row(
        "SELECT id, school_year, grade, name, created_at FROM classes WHERE id = ?1",
        [id],
        |r| Ok(ClassInfo { id: r.get(0)?, school_year: r.get(1)?, grade: r.get(2)?, name: r.get(3)?, created_at: r.get(4)? }),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "학급을 찾을 수 없습니다".to_string(),
        e => format!("데이터베이스 오류: {e}"),
    })
}

pub fn list(conn: &Connection) -> DbResult<Vec<ClassInfo>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, school_year, grade, name, created_at FROM classes
             WHERE archived_at IS NULL ORDER BY school_year, grade, name",
        )
        .db()?;
    let rows = stmt
        .query_map([], |r| Ok(ClassInfo { id: r.get(0)?, school_year: r.get(1)?, grade: r.get(2)?, name: r.get(3)?, created_at: r.get(4)? }))
        .db()?;
    rows.collect::<Result<_, _>>().db()
}

pub fn create(conn: &Connection, school_year: i64, grade: i64, name: &str) -> DbResult<ClassInfo> {
    let name = validate(grade, name)?;
    let id = new_id();
    conn.execute(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, school_year, grade, name, now_iso()],
    )
    .db()?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, school_year: i64, grade: i64, name: &str) -> DbResult<ClassInfo> {
    let name = validate(grade, name)?;
    let n = conn
        .execute(
            "UPDATE classes SET school_year = ?2, grade = ?3, name = ?4 WHERE id = ?1",
            params![id, school_year, grade, name],
        )
        .db()?;
    if n == 0 {
        return Err("학급을 찾을 수 없습니다".to_string());
    }
    get(conn, id)
}

pub fn archive(conn: &Connection, id: &str) -> DbResult<()> {
    let n = conn.execute("UPDATE classes SET archived_at = ?2 WHERE id = ?1", params![id, now_iso()]).db()?;
    if n == 0 {
        return Err("학급을 찾을 수 없습니다".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn class_list(db: State<Mutex<Connection>>) -> DbResult<Vec<ClassInfo>> {
    list(&*lock(&db)?)
}

#[tauri::command]
pub fn class_create(db: State<Mutex<Connection>>, school_year: i64, grade: i64, name: String) -> DbResult<ClassInfo> {
    create(&*lock(&db)?, school_year, grade, &name)
}

#[tauri::command]
pub fn class_update(db: State<Mutex<Connection>>, id: String, school_year: i64, grade: i64, name: String) -> DbResult<ClassInfo> {
    update(&*lock(&db)?, &id, school_year, grade, &name)
}

#[tauri::command]
pub fn class_archive(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    archive(&*lock(&db)?, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    #[test]
    fn create_list_update_archive() {
        let conn = test_conn();
        let b = create(&conn, 2027, 1, " 2반 ").unwrap();
        let a = create(&conn, 2027, 1, "1반").unwrap();
        assert_eq!(b.name, "2반");
        let names: Vec<_> = list(&conn).unwrap().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["1반", "2반"]);
        let u = update(&conn, &a.id, 2028, 2, "3반").unwrap();
        assert_eq!((u.school_year, u.grade, u.name.as_str()), (2028, 2, "3반"));
        archive(&conn, &b.id).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 1);
        assert!(create(&conn, 2027, 3, "x").is_err());
        assert!(create(&conn, 2027, 1, "  ").is_err());
    }
}
