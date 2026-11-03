//! 첨부 파일(사진, 음성 메모). 파일은 `media/YYYY/MM/<uuid>.<ext>`에 둔다.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::State;

use super::{lock, new_id, now_iso, DbExt, DbResult, MediaDir};

pub const MAX_PHOTO_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_AUDIO_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub kind: String,
    pub rel_path: String,
    pub abs_path: String,
    pub mime: String,
    pub bytes: i64,
    pub transcript: Option<String>,
    /// none | queued | running | done | failed
    pub transcript_status: String,
    pub transcript_error: Option<String>,
    pub transcript_edited: bool,
    pub transcript_model: Option<String>,
    pub created_at: String,
}

const SELECT: &str = "SELECT id, kind, rel_path, mime, bytes, transcript, created_at,
                      transcript_status, transcript_error, transcript_edited, transcript_model FROM attachments";

fn from_row(media: &Path, r: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    let rel: String = r.get(2)?;
    Ok(Attachment {
        id: r.get(0)?,
        kind: r.get(1)?,
        abs_path: abs_path(media, &rel).to_string_lossy().into_owned(),
        rel_path: rel,
        mime: r.get(3)?,
        bytes: r.get(4)?,
        transcript: r.get(5)?,
        created_at: r.get(6)?,
        transcript_status: r.get(7)?,
        transcript_error: r.get(8)?,
        transcript_edited: r.get::<_, i64>(9)? != 0,
        transcript_model: r.get(10)?,
    })
}

pub fn get(conn: &Connection, media: &Path, id: &str) -> DbResult<Attachment> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], |r| from_row(media, r))
        .map_err(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                "첨부를 찾을 수 없습니다".to_string()
            } else {
                format!("데이터베이스 오류: {e}")
            }
        })
}

pub fn abs_path(media: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(media.to_path_buf(), |p, part| p.join(part))
}

pub fn list_for(conn: &Connection, media: &Path, observation_id: &str) -> DbResult<Vec<Attachment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE observation_id = ?1 ORDER BY created_at, id")).db()?;
    let rows = stmt.query_map([observation_id], |r| from_row(media, r)).db()?;
    rows.collect::<Result<_, _>>().db()
}

fn photo_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "heic" => Some("image/heic"),
        _ => None,
    }
}

/// 녹음 MIME(`audio/webm;codecs=opus` 등)에서 확장자를 정한다.
pub fn audio_ext(mime: &str) -> Option<&'static str> {
    let base = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match base.as_str() {
        "audio/webm" => Some("webm"),
        "audio/ogg" => Some("ogg"),
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" | "audio/aac" => Some("m4a"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        _ => None,
    }
}

fn ensure_observation(conn: &Connection, id: &str) -> DbResult<()> {
    let deleted: Option<Option<String>> = conn
        .query_row("SELECT deleted_at FROM observations WHERE id = ?1", [id], |r| r.get(0))
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        .db()?;
    match deleted {
        None => Err("관찰 기록을 찾을 수 없습니다".to_string()),
        Some(Some(_)) => Err("삭제된 기록에는 첨부할 수 없습니다".to_string()),
        Some(None) => Ok(()),
    }
}

/// 새 파일 경로(상대, 절대)를 만들고 상위 폴더를 만든다.
fn new_target(media: &Path, ext: &str) -> DbResult<(String, PathBuf)> {
    let now = chrono::Local::now();
    let rel = format!("{}/{}.{}", now.format("%Y/%m"), new_id(), ext);
    let abs = abs_path(media, &rel);
    if let Some(dir) = abs.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("첨부 폴더를 만들 수 없습니다: {e}"))?;
    }
    Ok((rel, abs))
}

fn insert(conn: &Connection, media: &Path, observation_id: &str, kind: &str, rel: &str, mime: &str, bytes: u64) -> DbResult<Attachment> {
    let id = new_id();
    conn.execute(
        "INSERT INTO attachments (id, observation_id, kind, rel_path, mime, bytes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, observation_id, kind, rel, mime, bytes as i64, now_iso()],
    )
    .db()?;
    get(conn, media, &id)
}

pub fn add_file(conn: &Connection, media: &Path, observation_id: &str, path: &str) -> DbResult<Attachment> {
    ensure_observation(conn, observation_id)?;
    let src = Path::new(path);
    let ext = src.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let mime = photo_mime(&ext).ok_or("사진은 jpg, png, webp, heic 파일만 추가할 수 있습니다")?;
    let meta = std::fs::metadata(src).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
    if !meta.is_file() {
        return Err("파일을 선택해 주세요".to_string());
    }
    if meta.len() > MAX_PHOTO_BYTES {
        return Err("사진 파일이 너무 큽니다(50MB 이하만 가능)".to_string());
    }
    let (rel, abs) = new_target(media, &ext)?;
    std::fs::copy(src, &abs).map_err(|e| format!("파일을 복사하지 못했습니다: {e}"))?;
    insert(conn, media, observation_id, "photo", &rel, mime, meta.len()).inspect_err(|_| {
        let _ = std::fs::remove_file(&abs);
    })
}

pub fn add_bytes(conn: &Connection, media: &Path, observation_id: &str, kind: &str, mime: &str, data: &[u8]) -> DbResult<Attachment> {
    ensure_observation(conn, observation_id)?;
    if kind != "audio" {
        return Err("바이트 첨부는 음성(audio)만 지원합니다".to_string());
    }
    let ext = audio_ext(mime).ok_or("지원하지 않는 음성 형식입니다")?;
    if data.is_empty() {
        return Err("녹음된 내용이 없습니다".to_string());
    }
    if data.len() > MAX_AUDIO_BYTES {
        return Err("녹음 파일이 너무 큽니다(100MB 이하만 가능)".to_string());
    }
    let (rel, abs) = new_target(media, ext)?;
    std::fs::write(&abs, data).map_err(|e| format!("녹음을 저장하지 못했습니다: {e}"))?;
    insert(conn, media, observation_id, "audio", &rel, mime, data.len() as u64).inspect_err(|_| {
        let _ = std::fs::remove_file(&abs);
    })
}

pub fn delete(conn: &Connection, media: &Path, id: &str) -> DbResult<()> {
    let rel: String = conn
        .query_row("SELECT rel_path FROM attachments WHERE id = ?1", [id], |r| r.get(0))
        .map_err(|_| "첨부를 찾을 수 없습니다".to_string())?;
    conn.execute("DELETE FROM attachments WHERE id = ?1", [id]).db()?;
    let _ = std::fs::remove_file(abs_path(media, &rel));
    Ok(())
}

/// 교사가 자막을 고친다. 빈 글이면 자막을 지우고 처음 상태(`none`)로 돌린다.
pub fn set_transcript(conn: &Connection, media: &Path, id: &str, text: &str) -> DbResult<Attachment> {
    let a = get(conn, media, id)?;
    if a.kind != "audio" {
        return Err("음성 메모만 자막을 고칠 수 있습니다".to_string());
    }
    if a.transcript_status == "queued" || a.transcript_status == "running" {
        return Err("자막을 만드는 중에는 고칠 수 없습니다. 끝난 뒤에 고쳐 주세요".to_string());
    }
    let text = text.trim();
    if text.is_empty() {
        conn.execute(
            "UPDATE attachments SET transcript = NULL, transcript_status = 'none', transcript_error = NULL,
               transcript_model = NULL, transcript_edited = 0, transcribed_at = NULL WHERE id = ?1",
            [id],
        )
        .db()?;
    } else {
        conn.execute(
            "UPDATE attachments SET transcript = ?2, transcript_status = 'done', transcript_error = NULL,
               transcript_edited = 1 WHERE id = ?1",
            params![id, text],
        )
        .db()?;
    }
    get(conn, media, id)
}

#[tauri::command]
pub fn attachment_get(db: State<Mutex<Connection>>, media: State<MediaDir>, id: String) -> DbResult<Attachment> {
    get(&*lock(&db)?, &media.0, &id)
}

#[tauri::command]
pub fn attachment_set_transcript(db: State<Mutex<Connection>>, media: State<MediaDir>, id: String, text: String) -> DbResult<Attachment> {
    set_transcript(&*lock(&db)?, &media.0, &id, &text)
}

#[tauri::command]
pub fn attachment_add_file(db: State<Mutex<Connection>>, media: State<MediaDir>, observation_id: String, path: String) -> DbResult<Attachment> {
    add_file(&*lock(&db)?, &media.0, &observation_id, &path)
}

fn header(request: &Request<'_>, name: &str) -> DbResult<String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| format!("요청 헤더 {name}이(가) 없습니다"))
}

/// 큰 녹음을 JSON 배열로 보내지 않도록 원시 바이트 본문으로 받는다.
#[tauri::command]
pub fn attachment_add_bytes(request: Request<'_>, db: State<Mutex<Connection>>, media: State<MediaDir>) -> DbResult<Attachment> {
    let InvokeBody::Raw(data) = request.body() else {
        return Err("원시 바이트 본문이 필요합니다".to_string());
    };
    let observation_id = header(&request, "x-observation-id")?;
    let kind = header(&request, "x-kind")?;
    let mime = header(&request, "x-mime")?;
    add_bytes(&*lock(&db)?, &media.0, &observation_id, &kind, &mime, data)
}

#[tauri::command]
pub fn attachment_delete(db: State<Mutex<Connection>>, media: State<MediaDir>, id: String) -> DbResult<()> {
    delete(&*lock(&db)?, &media.0, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::observations::tests::seed;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-test-{}-{}", name, new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn audio_extension_from_mime() {
        assert_eq!(audio_ext("audio/webm;codecs=opus"), Some("webm"));
        assert_eq!(audio_ext("audio/ogg"), Some("ogg"));
        assert_eq!(audio_ext("audio/mp4"), Some("m4a"));
        assert_eq!(audio_ext("audio/wav"), Some("wav"));
        assert_eq!(audio_ext("text/plain"), None);
    }

    #[test]
    fn add_bytes_and_file_and_delete() {
        let (conn, _class, _students, obs) = seed();
        let media = tmp("media");
        let a = add_bytes(&conn, &media, &obs, "audio", "audio/webm;codecs=opus", b"abc").unwrap();
        assert_eq!((a.kind.as_str(), a.bytes), ("audio", 3));
        assert!(Path::new(&a.abs_path).exists());
        assert!(a.rel_path.ends_with(".webm") && a.rel_path.matches('/').count() == 2);
        assert!(add_bytes(&conn, &media, &obs, "audio", "audio/webm", b"").is_err());
        assert!(add_bytes(&conn, &media, &obs, "photo", "audio/webm", b"x").is_err());

        let src_dir = tmp("src");
        let photo = src_dir.join("A.JPG");
        std::fs::write(&photo, b"img").unwrap();
        let p = add_file(&conn, &media, &obs, photo.to_str().unwrap()).unwrap();
        assert_eq!(p.mime, "image/jpeg");
        assert!(photo.exists() && Path::new(&p.abs_path).exists());
        let txt = src_dir.join("a.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(add_file(&conn, &media, &obs, txt.to_str().unwrap()).is_err());
        assert!(add_file(&conn, &media, "nope", photo.to_str().unwrap()).is_err());

        assert_eq!(list_for(&conn, &media, &obs).unwrap().len(), 2);
        delete(&conn, &media, &a.id).unwrap();
        assert!(!Path::new(&a.abs_path).exists());
        assert_eq!(list_for(&conn, &media, &obs).unwrap().len(), 1);
    }

    #[test]
    fn set_transcript_marks_edited_and_empty_clears() {
        let (conn, _class, _students, obs) = seed();
        let media = tmp("media-tr");
        let a = add_bytes(&conn, &media, &obs, "audio", "audio/webm", b"abc").unwrap();
        assert_eq!((a.transcript_status.as_str(), a.transcript_edited), ("none", false));

        let b = set_transcript(&conn, &media, &a.id, "  고친 글 \n둘째 줄 ").unwrap();
        assert_eq!(b.transcript.as_deref(), Some("고친 글 \n둘째 줄"));
        assert_eq!((b.transcript_status.as_str(), b.transcript_edited), ("done", true));

        let c = set_transcript(&conn, &media, &a.id, "   ").unwrap();
        assert_eq!((c.transcript, c.transcript_status.as_str(), c.transcript_edited), (None, "none", false));

        conn.execute("UPDATE attachments SET transcript_status = 'running' WHERE id = ?1", [&a.id]).unwrap();
        assert!(set_transcript(&conn, &media, &a.id, "x").is_err());

        let src = tmp("src-tr").join("a.png");
        std::fs::write(&src, b"img").unwrap();
        let p = add_file(&conn, &media, &obs, src.to_str().unwrap()).unwrap();
        assert!(set_transcript(&conn, &media, &p.id, "x").is_err());
        assert!(set_transcript(&conn, &media, "nope", "x").is_err());
    }
}
