//! 칠판 타이머 안내 음성: 선생님이 직접 녹음한 음성을 설정 표(`voice.<id>`)에 base64 WAV로 둔다.
//! 교사 PC의 DB에 있으므로 백업에 함께 들어가고 암호화된다.

use std::sync::Mutex;

use rusqlite::Connection;
use serde_json::Value;

use crate::db::{lock, settings, DbExt, DbResult};
use crate::stt::validate_wav;

/// 문구 목록(`data/voice/cues.json`)을 빌드 때 포함한다.
const CUES_JSON: &str = include_str!("../../data/voice/cues.json");
/// 녹음 최대 길이(초)와 크기(바이트).
pub const MAX_SECONDS: f64 = 10.0;
pub const MAX_BYTES: usize = 1024 * 1024;

fn cue_ids() -> Vec<String> {
    let v: Value = serde_json::from_str(CUES_JSON).unwrap_or(Value::Null);
    v["cues"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c["id"].as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn key_of(id: &str) -> DbResult<String> {
    if cue_ids().iter().any(|c| c == id) {
        Ok(format!("voice.{id}"))
    } else {
        Err("알 수 없는 안내 음성입니다".to_string())
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    const BAD: &str = "저장된 안내 음성을 읽을 수 없습니다";
    let bytes = s.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(BAD.to_string());
    }
    let val = |c: u8| B64.iter().position(|&b| b == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return Err(BAD.to_string());
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n <<= 6;
            if i < 4 - pad {
                n |= val(c).ok_or_else(|| BAD.to_string())?;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// 저장된 녹음. 없으면 `None`(기본 음성을 쓴다).
pub fn get(conn: &Connection, id: &str) -> DbResult<Option<Vec<u8>>> {
    match settings::get(conn, &key_of(id)?)? {
        Value::String(s) => b64_decode(&s).map(Some),
        _ => Ok(None),
    }
}

pub fn set(conn: &Connection, id: &str, wav: &[u8]) -> DbResult<()> {
    let key = key_of(id)?;
    if wav.len() > MAX_BYTES {
        return Err("녹음 파일이 너무 큽니다(1MB 이하)".to_string());
    }
    let seconds = validate_wav(wav)?;
    if seconds > MAX_SECONDS {
        return Err("안내 음성은 10초 이하로 녹음해 주세요".to_string());
    }
    settings::set(conn, &key, &Value::String(b64_encode(wav)))?;
    Ok(())
}

/// 선생님 녹음을 지워 기본 음성으로 되돌린다. 없어도 오류가 아니다.
pub fn clear(conn: &Connection, id: &str) -> DbResult<()> {
    let key = key_of(id)?;
    conn.execute("DELETE FROM settings WHERE key = ?1", [&key]).db()?;
    Ok(())
}

#[tauri::command]
pub fn voice_get(db: tauri::State<Mutex<Connection>>, id: String) -> DbResult<Option<Vec<u8>>> {
    get(&*lock(&db)?, &id)
}

#[tauri::command]
pub fn voice_set(db: tauri::State<Mutex<Connection>>, id: String, wav: Vec<u8>) -> DbResult<()> {
    set(&*lock(&db)?, &id, &wav)
}

#[tauri::command]
pub fn voice_clear(db: tauri::State<Mutex<Connection>>, id: String) -> DbResult<()> {
    clear(&*lock(&db)?, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    fn wav(rate: u32, channels: u16, data_len: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(channels * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data_len as u32).to_le_bytes());
        v.extend((0..data_len).map(|i| (i % 251) as u8));
        v
    }

    #[test]
    fn base64_roundtrip_all_lengths() {
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"Ma"), "TWE=");
        assert_eq!(b64_encode(b"M"), "TQ==");
        for n in 0..40usize {
            let d: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(b64_decode(&b64_encode(&d)).unwrap(), d);
        }
        assert!(b64_decode("abc").is_err());
        assert!(b64_decode("ab!d").is_err());
    }

    #[test]
    fn set_get_roundtrip_and_clear() {
        let conn = test_conn();
        assert_eq!(get(&conn, "timer-end").unwrap(), None);
        let w = wav(16000, 1, 32000 * 2);
        set(&conn, "timer-end", &w).unwrap();
        assert_eq!(get(&conn, "timer-end").unwrap().unwrap(), w);
        assert_eq!(get(&conn, "timer-30s").unwrap(), None);
        // 덮어쓰기
        let w2 = wav(16000, 1, 32000);
        set(&conn, "timer-end", &w2).unwrap();
        assert_eq!(get(&conn, "timer-end").unwrap().unwrap(), w2);
        clear(&conn, "timer-end").unwrap();
        assert_eq!(get(&conn, "timer-end").unwrap(), None);
        clear(&conn, "timer-end").unwrap();
    }

    #[test]
    fn set_rejects_bad_input() {
        let conn = test_conn();
        let ok = wav(16000, 1, 32000);
        assert!(set(&conn, "nope", &ok).unwrap_err().contains("알 수 없는"));
        assert!(get(&conn, "../x").is_err());
        assert!(clear(&conn, "nope").is_err());
        assert!(set(&conn, "timer-1min", b"not a wav").is_err());
        assert!(set(&conn, "timer-1min", &wav(44100, 1, 1000)).unwrap_err().contains("16kHz"));
        assert!(set(&conn, "timer-1min", &wav(16000, 2, 1000)).unwrap_err().contains("모노"));
        // 10초 경계: 10초는 통과, 10초보다 길면 거부
        assert!(set(&conn, "timer-1min", &wav(16000, 1, 32000 * 10)).is_ok());
        assert!(set(&conn, "timer-1min", &wav(16000, 1, 32000 * 10 + 2)).unwrap_err().contains("10초"));
        // 1MB 초과
        assert!(set(&conn, "timer-30s", &vec![0u8; MAX_BYTES + 1]).unwrap_err().contains("1MB"));
        assert_eq!(get(&conn, "timer-30s").unwrap(), None);
    }

    #[test]
    fn cue_ids_come_from_cues_json() {
        let ids = cue_ids();
        assert_eq!(ids, ["timer-1min", "timer-30s", "timer-end"]);
    }
}
