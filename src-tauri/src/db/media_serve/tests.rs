use super::*;
use crate::db::attachments;
use crate::db::media::{self, MediaKey};
use crate::db::new_id;
use crate::db::observations::tests::seed;

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tonghap-serve-{}-{}", name, new_id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn key() -> MediaKey {
    MediaKey::for_test(1)
}

fn get(conn: &Connection, media: &Path, k: Option<&MediaKey>, id: &str, range: Option<&str>) -> Response<Vec<u8>> {
    serve(conn, media, k, "GET", &format!("/{id}"), range)
}

fn header_of<'a>(r: &'a Response<Vec<u8>>, name: &str) -> &'a str {
    r.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

const AUDIO: &[u8] = b"0123456789abcdefghij";

fn setup() -> (Connection, std::path::PathBuf, String, String) {
    let (conn, _c, _s, obs) = seed();
    let media = tmp("media");
    let a = attachments::add_bytes(&conn, &media, &key(), &obs, "audio", "audio/webm;codecs=opus", AUDIO).unwrap();
    let src = tmp("src").join("p.PNG");
    std::fs::write(&src, b"\x89PNG....photo").unwrap();
    let p = attachments::add_file(&conn, &media, &key(), &obs, src.to_str().unwrap()).unwrap();
    (conn, media, a.id, p.id)
}

#[test]
fn serves_decrypted_content_by_id_with_mime_and_headers() {
    let (conn, media, audio, photo) = setup();
    // 디스크에는 암호문
    let rel: String = conn.query_row("SELECT rel_path FROM attachments WHERE id = ?1", [&audio], |r| r.get(0)).unwrap();
    let disk = std::fs::read(attachments::abs_path(&media, &rel)).unwrap();
    assert_eq!(&disk[..4], b"TGM1");

    let r = get(&conn, &media, Some(&key()), &audio, None);
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.body().as_slice(), AUDIO);
    assert_eq!(header_of(&r, "content-type"), "audio/webm;codecs=opus");
    assert_eq!(header_of(&r, "cache-control"), "no-store");
    assert_eq!(header_of(&r, "accept-ranges"), "bytes");
    assert_eq!(header_of(&r, "access-control-allow-origin"), "*");
    assert_eq!(header_of(&r, "content-length"), "20");

    let p = get(&conn, &media, Some(&key()), &photo, None);
    assert_eq!(p.status(), StatusCode::OK);
    assert_eq!(header_of(&p, "content-type"), "image/png");
    assert_eq!(p.body().as_slice(), b"\x89PNG....photo");
}

#[test]
fn legacy_plain_files_are_served_too() {
    let (conn, media, audio, _) = setup();
    let rel: String = conn.query_row("SELECT rel_path FROM attachments WHERE id = ?1", [&audio], |r| r.get(0)).unwrap();
    std::fs::write(attachments::abs_path(&media, &rel), AUDIO).unwrap();
    let r = get(&conn, &media, Some(&key()), &audio, None);
    assert_eq!((r.status(), r.body().as_slice()), (StatusCode::OK, AUDIO));
    // 키가 없어도 평문은 열린다
    assert_eq!(get(&conn, &media, None, &audio, None).status(), StatusCode::OK);
}

#[test]
fn unknown_and_malformed_ids_are_404() {
    let (conn, media, _, _) = setup();
    for path in ["/nope", "/", "", "/../x", "/..%2F..%2Fetc%2Fpasswd", "/a/b", "/%2e%2e", "/x y", "/\\windows\\win.ini", "/C:/windows/win.ini"] {
        let r = serve(&conn, &media, Some(&key()), "GET", path, None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[test]
fn rel_path_outside_media_folder_is_refused() {
    let (conn, media, audio, _) = setup();
    let outside = tmp("outside");
    std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
    // media 옆 폴더를 가리키는 값으로 바꿔 본다
    let rel_out = format!("../{}/secret.txt", outside.file_name().unwrap().to_string_lossy());
    for bad in [rel_out.as_str(), "../secret.txt", "/etc/passwd", "a/../../secret.txt", "..\\secret.txt", "C:\\x"] {
        conn.execute("UPDATE attachments SET rel_path = ?2 WHERE id = ?1", rusqlite::params![audio, bad]).unwrap();
        let r = get(&conn, &media, Some(&key()), &audio, None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{bad}");
        assert!(!r.body().windows(6).any(|w| w == b"secret"));
    }
}

#[cfg(unix)]
#[test]
fn symlink_escaping_media_folder_is_refused() {
    let (conn, media, audio, _) = setup();
    let outside = tmp("outside-link");
    std::fs::write(outside.join("s.webm"), b"secret-bytes").unwrap();
    std::os::unix::fs::symlink(&outside, media.join("link")).unwrap();
    conn.execute("UPDATE attachments SET rel_path = 'link/s.webm' WHERE id = ?1", [&audio]).unwrap();
    assert_eq!(get(&conn, &media, Some(&key()), &audio, None).status(), StatusCode::NOT_FOUND);
}

#[test]
fn missing_file_is_404() {
    let (conn, media, audio, _) = setup();
    let rel: String = conn.query_row("SELECT rel_path FROM attachments WHERE id = ?1", [&audio], |r| r.get(0)).unwrap();
    std::fs::remove_file(attachments::abs_path(&media, &rel)).unwrap();
    assert_eq!(get(&conn, &media, Some(&key()), &audio, None).status(), StatusCode::NOT_FOUND);
}

#[test]
fn range_requests_return_206() {
    let (conn, media, audio, _) = setup();
    let k = key();
    let r = get(&conn, &media, Some(&k), &audio, Some("bytes=2-5"));
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.body().as_slice(), b"2345");
    assert_eq!(header_of(&r, "content-range"), "bytes 2-5/20");
    assert_eq!(header_of(&r, "content-length"), "4");
    assert_eq!(header_of(&r, "content-type"), "audio/webm;codecs=opus");

    let open = get(&conn, &media, Some(&k), &audio, Some("bytes=15-"));
    assert_eq!((open.status(), open.body().as_slice()), (StatusCode::PARTIAL_CONTENT, &AUDIO[15..]));
    assert_eq!(header_of(&open, "content-range"), "bytes 15-19/20");

    let suffix = get(&conn, &media, Some(&k), &audio, Some("bytes=-4"));
    assert_eq!(suffix.body().as_slice(), &AUDIO[16..]);

    let clipped = get(&conn, &media, Some(&k), &audio, Some("bytes=18-999"));
    assert_eq!((clipped.status(), clipped.body().as_slice()), (StatusCode::PARTIAL_CONTENT, &AUDIO[18..]));

    let zero = get(&conn, &media, Some(&k), &audio, Some("bytes=0-0"));
    assert_eq!(zero.body().as_slice(), b"0");

    for bad in ["bytes=20-", "bytes=5-2", "bytes=x-y", "bytes=0-1,5-6", "bytes=-0"] {
        let r = get(&conn, &media, Some(&k), &audio, Some(bad));
        assert_eq!(r.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{bad}");
        assert_eq!(header_of(&r, "content-range"), "bytes */20");
    }
    // 이해하지 못하는 단위는 전체를 준다
    assert_eq!(get(&conn, &media, Some(&k), &audio, Some("items=1-2")).status(), StatusCode::OK);
}

#[test]
fn head_options_and_other_methods() {
    let (conn, media, audio, _) = setup();
    let h = serve(&conn, &media, Some(&key()), "HEAD", &format!("/{audio}"), None);
    assert_eq!(h.status(), StatusCode::OK);
    assert!(h.body().is_empty());
    assert_eq!(header_of(&h, "content-length"), "20");
    let o = serve(&conn, &media, Some(&key()), "OPTIONS", &format!("/{audio}"), None);
    assert_eq!(o.status(), StatusCode::NO_CONTENT);
    assert!(header_of(&o, "access-control-allow-headers").contains("Range"));
    for m in ["POST", "PUT", "DELETE"] {
        assert_eq!(serve(&conn, &media, Some(&key()), m, &format!("/{audio}"), None).status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}

#[test]
fn encrypted_without_key_swapped_or_tampered_files_are_not_served() {
    let (conn, media, audio, photo) = setup();
    assert_eq!(get(&conn, &media, None, &audio, None).status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(get(&conn, &media, Some(&MediaKey::for_test(2)), &audio, None).status(), StatusCode::INTERNAL_SERVER_ERROR);

    // 한 첨부의 파일을 다른 첨부 자리로 바꿔치기 → AAD가 달라 열리지 않는다
    let rel_of = |id: &str| -> String { conn.query_row("SELECT rel_path FROM attachments WHERE id = ?1", [id], |r| r.get(0)).unwrap() };
    let (pa, pp) = (attachments::abs_path(&media, &rel_of(&audio)), attachments::abs_path(&media, &rel_of(&photo)));
    std::fs::copy(&pa, &pp).unwrap();
    assert_eq!(get(&conn, &media, Some(&key()), &photo, None).status(), StatusCode::INTERNAL_SERVER_ERROR);

    // 손상
    let mut d = std::fs::read(&pa).unwrap();
    let last = d.len() - 1;
    d[last] ^= 1;
    std::fs::write(&pa, d).unwrap();
    let r = get(&conn, &media, Some(&key()), &audio, None);
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!r.body().windows(4).any(|w| w == b"0123"));
}

#[test]
fn stored_attachment_bytes_column_is_plain_length() {
    let (conn, _media, audio, _) = setup();
    let n: i64 = conn.query_row("SELECT bytes FROM attachments WHERE id = ?1", [&audio], |r| r.get(0)).unwrap();
    assert_eq!(n, AUDIO.len() as i64);
    let _ = media::MAX_FILE_BYTES;
}
