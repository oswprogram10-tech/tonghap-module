//! 앱 설정(키-값, 값은 JSON).

use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde_json::Value;
use tauri::State;

use super::{lock, DbExt, DbResult};

/// 없는 키는 `null`.
pub fn get(conn: &Connection, key: &str) -> DbResult<Value> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        .db()?;
    match raw {
        None => Ok(Value::Null),
        Some(s) => serde_json::from_str(&s).map_err(|e| format!("설정 값을 읽을 수 없습니다: {e}")),
    }
}

pub fn set(conn: &Connection, key: &str, value: &Value) -> DbResult<Value> {
    if key.trim().is_empty() {
        return Err("설정 키가 비어 있습니다".to_string());
    }
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )
    .db()?;
    Ok(value.clone())
}

#[tauri::command]
pub fn settings_get(db: State<Mutex<Connection>>, key: String) -> DbResult<Value> {
    get(&*lock(&db)?, &key)
}

#[tauri::command]
pub fn settings_set(db: State<Mutex<Connection>>, key: String, value: Value) -> DbResult<Value> {
    set(&*lock(&db)?, &key, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    #[test]
    fn roundtrip_and_overwrite() {
        let conn = test_conn();
        assert_eq!(get(&conn, "currentClassId").unwrap(), Value::Null);
        set(&conn, "currentClassId", &serde_json::json!("abc")).unwrap();
        assert_eq!(get(&conn, "currentClassId").unwrap(), "abc");
        set(&conn, "currentClassId", &serde_json::json!({"a": [1, 2]})).unwrap();
        assert_eq!(get(&conn, "currentClassId").unwrap()["a"][1], 2);
    }
}
