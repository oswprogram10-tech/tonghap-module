//! 칠판 모드의 사용자 놀이 카드와 놀이 기록.
//!
//! 놀이 카드의 검증 규칙은 `data/plays/schema/plays.schema.json`과 같다.

use std::sync::Mutex;

use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{classes, lock, new_id, now_iso, DbExt, DbResult};

const PLACES: [&str; 3] = ["outdoor", "indoor", "both"];
const GROUPS: [&str; 4] = ["whole", "teams", "pairs", "small"];
const TAGS: [&str; 6] = ["전래", "신체", "협동", "표현", "규칙", "조용한"];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Play {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub place: String,
    pub group: String,
    pub minutes: i64,
    pub rules: Vec<String>,
    pub safety: Vec<String>,
    #[serde(default)]
    pub materials: Vec<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub topic_book_ids: Vec<String>,
}

fn check_list(items: &[String], what: &str, min: usize, max: usize, min_len: usize, max_len: usize) -> DbResult<()> {
    if items.len() < min || items.len() > max {
        return Err(format!("{what}은(는) {min}개 이상 {max}개 이하로 적어 주세요"));
    }
    for t in items {
        let n = t.trim().chars().count();
        if n < min_len || n > max_len {
            return Err(format!("{what} 한 줄은 {min_len}~{max_len}자로 적어 주세요"));
        }
    }
    Ok(())
}

/// 스키마와 같은 규칙으로 검사하고, 공백을 다듬은 카드를 돌려준다.
pub fn validate(play: &Play) -> DbResult<Play> {
    let mut p = play.clone();
    p.name = p.name.trim().to_string();
    let n = p.name.chars().count();
    if n == 0 || n > 20 {
        return Err("놀이 이름은 1~20자로 적어 주세요".to_string());
    }
    if !PLACES.contains(&p.place.as_str()) {
        return Err("장소는 실외·실내·둘 다 중에서 골라 주세요".to_string());
    }
    if !GROUPS.contains(&p.group.as_str()) {
        return Err("인원 구성을 올바르게 골라 주세요".to_string());
    }
    if !(5..=40).contains(&p.minutes) {
        return Err("놀이 시간은 5~40분이어야 합니다".to_string());
    }
    p.rules = p.rules.iter().map(|s| s.trim().to_string()).collect();
    p.safety = p.safety.iter().map(|s| s.trim().to_string()).collect();
    p.materials = p.materials.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    check_list(&p.rules, "규칙", 2, 5, 2, 40)?;
    check_list(&p.safety, "안전 수칙", 1, 3, 2, 30)?;
    if p.materials.iter().any(|m| m.chars().count() > 20) {
        return Err("준비물 한 개는 20자 이하로 적어 주세요".to_string());
    }
    if p.tags.is_empty() || p.tags.iter().any(|t| !TAGS.contains(&t.as_str())) {
        return Err("놀이 종류 태그를 1개 이상 골라 주세요".to_string());
    }
    let re = Regex::new(r"^g[12]s[12]-[a-z0-9-]+$").map_err(|e| e.to_string())?;
    if p.topic_book_ids.iter().any(|t| !re.is_match(t)) {
        return Err("주제책 번호 형식이 올바르지 않습니다".to_string());
    }
    Ok(p)
}

fn parse(data: &str) -> DbResult<Play> {
    serde_json::from_str(data).map_err(|e| format!("저장된 놀이를 읽을 수 없습니다: {e}"))
}

pub fn list_custom(conn: &Connection) -> DbResult<Vec<Play>> {
    let mut stmt = conn.prepare("SELECT data FROM custom_plays ORDER BY created_at, id").db()?;
    let rows: Vec<String> = stmt.query_map([], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
    rows.iter().map(|d| parse(d)).collect()
}

/// id가 없으면 새로 만들고(`custom-<uuid>`), 있으면 고친다. 사용자 놀이만 고칠 수 있다.
pub fn save_custom(conn: &Connection, play: &Play) -> DbResult<Play> {
    let mut p = validate(play)?;
    let now = now_iso();
    if p.id.trim().is_empty() {
        p.id = format!("custom-{}", new_id());
        let data = serde_json::to_string(&p).map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO custom_plays (id, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)", params![p.id, data, now]).db()?;
    } else {
        if !p.id.starts_with("custom-") {
            return Err("기본 놀이는 고칠 수 없습니다".to_string());
        }
        let data = serde_json::to_string(&p).map_err(|e| e.to_string())?;
        let n = conn.execute("UPDATE custom_plays SET data = ?2, updated_at = ?3 WHERE id = ?1", params![p.id, data, now]).db()?;
        if n == 0 {
            return Err("놀이를 찾을 수 없습니다".to_string());
        }
    }
    Ok(p)
}

pub fn delete_custom(conn: &Connection, id: &str) -> DbResult<()> {
    let n = conn.execute("DELETE FROM custom_plays WHERE id = ?1", [id]).db()?;
    if n == 0 {
        return Err("놀이를 찾을 수 없습니다".to_string());
    }
    Ok(())
}

pub fn log_add(conn: &Connection, class_id: &str, play_id: &str, minutes: Option<i64>) -> DbResult<()> {
    classes::get(conn, class_id)?;
    if play_id.trim().is_empty() {
        return Err("놀이를 알 수 없습니다".to_string());
    }
    if minutes.is_some_and(|m| !(0..=600).contains(&m)) {
        return Err("놀이 시간이 올바르지 않습니다".to_string());
    }
    conn.execute(
        "INSERT INTO play_log (id, class_id, play_id, played_at, minutes) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![new_id(), class_id, play_id, now_iso(), minutes],
    )
    .db()?;
    Ok(())
}

#[tauri::command]
pub fn play_custom_list(db: State<Mutex<Connection>>) -> DbResult<Vec<Play>> {
    list_custom(&*lock(&db)?)
}

#[tauri::command]
pub fn play_custom_save(db: State<Mutex<Connection>>, play: Play) -> DbResult<Play> {
    save_custom(&*lock(&db)?, &play)
}

#[tauri::command]
pub fn play_custom_delete(db: State<Mutex<Connection>>, id: String) -> DbResult<()> {
    delete_custom(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn play_log_add(db: State<Mutex<Connection>>, class_id: String, play_id: String, minutes: Option<i64>) -> DbResult<()> {
    log_add(&*lock(&db)?, &class_id, &play_id, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    fn play() -> Play {
        Play {
            id: String::new(),
            name: "꼬리잡기".into(),
            place: "outdoor".into(),
            group: "teams".into(),
            minutes: 10,
            rules: vec!["줄을 서서 앞사람 허리를 잡는다".into(), "맨 앞 사람이 꼬리를 잡는다".into()],
            safety: vec!["허리를 세게 당기지 않는다".into()],
            materials: vec![],
            tags: vec!["신체".into()],
            topic_book_ids: vec!["g1s1-korea".into()],
        }
    }

    #[test]
    fn custom_play_save_update_delete() {
        let conn = test_conn();
        assert!(list_custom(&conn).unwrap().is_empty());
        let saved = save_custom(&conn, &play()).unwrap();
        assert!(saved.id.starts_with("custom-"));
        let mut edit = saved.clone();
        edit.name = "  새 꼬리잡기 ".into();
        edit.minutes = 15;
        let updated = save_custom(&conn, &edit).unwrap();
        assert_eq!((updated.id.as_str(), updated.name.as_str(), updated.minutes), (saved.id.as_str(), "새 꼬리잡기", 15));
        let all = list_custom(&conn).unwrap();
        assert_eq!(all, vec![updated]);
        delete_custom(&conn, &saved.id).unwrap();
        assert!(list_custom(&conn).unwrap().is_empty());
        assert!(delete_custom(&conn, &saved.id).is_err());
    }

    #[test]
    fn invalid_plays_are_rejected() {
        let conn = test_conn();
        let mut one_rule = play();
        one_rule.rules.truncate(1);
        assert!(save_custom(&conn, &one_rule).is_err());
        let mut long_name = play();
        long_name.name = "가".repeat(21);
        assert!(save_custom(&conn, &long_name).is_err());
        let mut bad_place = play();
        bad_place.place = "pool".into();
        assert!(save_custom(&conn, &bad_place).is_err());
        let mut no_safety = play();
        no_safety.safety.clear();
        assert!(save_custom(&conn, &no_safety).is_err());
        let mut bad_minutes = play();
        bad_minutes.minutes = 3;
        assert!(save_custom(&conn, &bad_minutes).is_err());
        let mut bad_tag = play();
        bad_tag.tags = vec!["없는태그".into()];
        assert!(save_custom(&conn, &bad_tag).is_err());
        let mut bad_topic = play();
        bad_topic.topic_book_ids = vec!["korea".into()];
        assert!(save_custom(&conn, &bad_topic).is_err());
        let mut builtin = play();
        builtin.id = "red-light-green-light".into();
        assert!(save_custom(&conn, &builtin).is_err());
        let mut missing = play();
        missing.id = "custom-none".into();
        assert!(save_custom(&conn, &missing).is_err());
        assert!(list_custom(&conn).unwrap().is_empty());
    }

    #[test]
    fn play_log_records_and_validates() {
        let conn = test_conn();
        let c = classes::create(&conn, 2027, 1, "1반").unwrap();
        log_add(&conn, &c.id, "mirror-play", Some(5)).unwrap();
        log_add(&conn, &c.id, "mirror-play", None).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM play_log WHERE class_id = ?1", [&c.id], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        assert!(log_add(&conn, "none", "mirror-play", None).is_err());
        assert!(log_add(&conn, &c.id, "", None).is_err());
        assert!(log_add(&conn, &c.id, "x", Some(-1)).is_err());
    }
}
