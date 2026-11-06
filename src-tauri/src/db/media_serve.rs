//! `tgmedia` 프로토콜: 화면이 사진·음성을 **첨부 id**로 요청하면 DB에서 경로를 찾아 복호화해 돌려준다.
//! 설계: docs/design/21-media-encryption.md 3절. 다른 파일은 읽을 수 없다(id가 아니면 404, 미디어 폴더 밖이면 거부).
//!
//! 순수 함수 `serve`가 본체이고, 앱 연결(`respond`)은 얇게 감싼다.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Manager};

use super::media::{self, MediaKey, MediaKeyHolder};
use super::MediaDir;

pub const SCHEME: &str = "tgmedia";

fn base(status: StatusCode) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Length, Content-Range, Accept-Ranges")
        .header("X-Content-Type-Options", "nosniff")
}

fn text(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    base(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(msg.as_bytes().to_vec())
        .expect("고정된 헤더로 응답을 만든다")
}

/// 주소 경로(`/<id>`)에서 첨부 id를 꺼낸다. 영문·숫자·`-`만 허용한다(퍼센트 인코딩 포함 그 밖은 거부).
fn parse_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix('/')?;
    (!id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).then_some(id)
}

/// `Range: bytes=a-b` 해석. `Ok(None)`은 범위 요청이 아님, `Err`은 만족할 수 없는 범위(416).
fn parse_range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(h) = header else { return Ok(None) };
    let Some(spec) = h.trim().strip_prefix("bytes=") else { return Ok(None) };
    if spec.contains(',') {
        return Err(());
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (len.saturating_sub(n), len.saturating_sub(1))
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e: u64 = if b.is_empty() { len.saturating_sub(1) } else { b.parse().map_err(|_| ())? };
        (s, e.min(len.saturating_sub(1)))
    };
    if len == 0 || start > end || start >= len {
        return Err(());
    }
    Ok(Some((start, end)))
}

/// 요청의 결과: 바로 줄 응답이거나, DB에서 찾은 파일 정보(복호화는 DB 잠금을 놓은 뒤에 한다).
pub enum Lookup {
    Done(Response<Vec<u8>>),
    File { rel: String, mime: String },
}

/// 1단계(DB 잠금 안에서): 메서드·주소를 검사하고 첨부 id로 경로와 형식을 찾는다.
pub fn lookup(conn: &Connection, method: &str, path: &str) -> Lookup {
    if method == "OPTIONS" {
        return Lookup::Done(
            base(StatusCode::NO_CONTENT)
                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
                .body(Vec::new())
                .expect("고정된 헤더로 응답을 만든다"),
        );
    }
    if method != "GET" && method != "HEAD" {
        return Lookup::Done(text(StatusCode::METHOD_NOT_ALLOWED, "허용되지 않는 요청입니다"));
    }
    let Some(id) = parse_id(path) else { return Lookup::Done(text(StatusCode::NOT_FOUND, "찾을 수 없습니다")) };
    let row: Option<(String, String)> = conn
        .query_row("SELECT rel_path, mime FROM attachments WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    match row {
        Some((rel, mime)) => Lookup::File { rel, mime },
        None => Lookup::Done(text(StatusCode::NOT_FOUND, "찾을 수 없습니다")),
    }
}

/// 2단계: 파일을 읽어 복호화하고 응답을 만든다.
pub fn serve_file(media_dir: &Path, key: Option<&MediaKey>, method: &str, rel: &str, mime: &str, range: Option<&str>) -> Response<Vec<u8>> {
    // 미디어 폴더 밖은 읽지 않는다(DB 값이 이상해도 막는다).
    let abs = super::attachments::abs_path(media_dir, rel);
    let inside = crate::backup::safe_rel(rel)
        && matches!((abs.canonicalize(), media_dir.canonicalize()), (Ok(a), Ok(m)) if a.starts_with(&m));
    if !inside {
        return text(StatusCode::NOT_FOUND, "찾을 수 없습니다");
    }
    let plain = match media::read_plain(key, &abs, rel) {
        Ok(p) => p,
        Err(e) => {
            crate::logging::error("media", &format!("첨부를 읽지 못했습니다: {e}"));
            return text(StatusCode::INTERNAL_SERVER_ERROR, "파일을 읽을 수 없습니다");
        }
    };
    let len = plain.len() as u64;
    let content_type = if mime.is_empty() { "application/octet-stream" } else { mime };
    let head_only = method == "HEAD";
    let body = |b: &[u8]| if head_only { Vec::new() } else { b.to_vec() };
    match parse_range(range, len) {
        Err(()) => base(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body("범위가 올바르지 않습니다".as_bytes().to_vec())
            .expect("고정된 헤더로 응답을 만든다"),
        Ok(Some((s, e))) => base(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes {s}-{e}/{len}"))
            .header(header::CONTENT_LENGTH, e - s + 1)
            .body(body(&plain[s as usize..=e as usize]))
            .expect("고정된 헤더로 응답을 만든다"),
        Ok(None) => base(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, len)
            .body(body(&plain))
            .expect("고정된 헤더로 응답을 만든다"),
    }
}

/// 한 번에 처리한다(테스트용).
#[cfg(test)]
pub fn serve(conn: &Connection, media_dir: &Path, key: Option<&MediaKey>, method: &str, path: &str, range: Option<&str>) -> Response<Vec<u8>> {
    match lookup(conn, method, path) {
        Lookup::Done(r) => r,
        Lookup::File { rel, mime } => serve_file(media_dir, key, method, &rel, &mime, range),
    }
}

/// 프로토콜 처리기에서 부른다(별도 스레드에서). 앱 상태에서 DB·폴더·키를 꺼내 처리한다.
/// DB 잠금은 경로를 찾는 동안에만 쥔다.
pub fn respond(app: &AppHandle, req: &tauri::http::Request<Vec<u8>>) -> Response<Vec<u8>> {
    let range = req.headers().get(header::RANGE).and_then(|v| v.to_str().ok());
    let method = req.method().as_str();
    let found = {
        let db = app.state::<Mutex<Connection>>();
        let Ok(conn) = super::lock(&db) else { return text(StatusCode::SERVICE_UNAVAILABLE, "데이터베이스가 잠겨 있습니다") };
        lookup(&conn, method, req.uri().path())
    };
    match found {
        Lookup::Done(r) => r,
        Lookup::File { rel, mime } => {
            let media = app.state::<MediaDir>();
            let key = app.state::<MediaKeyHolder>().get();
            serve_file(&media.0, key.as_ref(), method, &rel, &mime, range)
        }
    }
}

#[cfg(test)]
mod tests;
