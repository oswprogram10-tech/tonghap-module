use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use super::*;
use crate::db::new_id;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tonghap-media-{}-{}", name, new_id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4, 5, 6, 7, 8];

fn wav() -> Vec<u8> {
    let mut v = b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec();
    v.extend_from_slice(&[0u8; 40]);
    v
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// 폴더 안 모든 파일의 내용(상대 경로 → 바이트). tmp 파일 포함.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, cur: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(cur).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn put(dir: &Path, rel: &str, data: &[u8]) -> PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, data).unwrap();
    p
}

// ---------- 형식 ----------

#[test]
fn round_trip_and_format() {
    let key = MediaKey::for_test(1);
    let enc = encrypt(&key, "2027/03/a.png", PNG).unwrap();
    assert_eq!(&enc[..4], b"TGM1");
    assert_eq!(enc.len() as u64, PNG.len() as u64 + OVERHEAD);
    assert_eq!(decrypt(&key, "2027/03/a.png", &enc).unwrap(), PNG);
    // 같은 내용도 매번 다른 nonce
    assert_ne!(enc, encrypt(&key, "2027/03/a.png", PNG).unwrap());
    // 빈 파일도 왕복
    assert!(decrypt(&key, "x", &encrypt(&key, "x", b"").unwrap()).unwrap().is_empty());
}

#[test]
fn different_aad_wrong_key_and_corruption_fail() {
    let key = MediaKey::for_test(1);
    let enc = encrypt(&key, "2027/03/a.png", PNG).unwrap();
    assert!(decrypt(&key, "2027/03/b.png", &enc).is_err(), "다른 자리의 첨부로는 열리지 않는다");
    assert!(decrypt(&MediaKey::for_test(2), "2027/03/a.png", &enc).is_err());
    // 어느 한 바이트를 바꿔도 실패(nonce, 암호문, 태그)
    for i in [4, 10, 28, enc.len() / 2, enc.len() - 1] {
        let mut bad = enc.clone();
        bad[i] ^= 1;
        assert!(decrypt(&key, "2027/03/a.png", &bad).is_err(), "바이트 {i}");
    }
    // 잘림
    assert!(decrypt(&key, "2027/03/a.png", &enc[..enc.len() - 1]).is_err());
    assert!(decrypt(&key, "2027/03/a.png", &enc[..20]).is_err());
}

#[test]
fn plain_files_are_read_as_is_and_encrypted_need_a_key() {
    let key = MediaKey::for_test(1);
    assert_eq!(open_bytes(None, "a", PNG.to_vec()).unwrap(), PNG);
    assert_eq!(open_bytes(Some(&key), "a", PNG.to_vec()).unwrap(), PNG);
    let enc = encrypt(&key, "a", PNG).unwrap();
    assert!(open_bytes(None, "a", enc.clone()).is_err());
    assert_eq!(open_bytes(Some(&key), "a", enc).unwrap(), PNG);
}

#[test]
fn media_key_is_derived_not_the_db_key_and_stable() {
    let db = DbKey::from_bytes([9; KEY_LEN]);
    let a = db.derive_media_key();
    assert_eq!(a, db.derive_media_key());
    assert_ne!(a, [9; KEY_LEN]);
    assert_ne!(a, DbKey::from_bytes([8; KEY_LEN]).derive_media_key());
    assert!(!format!("{:?}", MediaKey::from_db_key(&db)).contains("09"));
}

#[test]
fn too_big_is_refused() {
    let key = MediaKey::for_test(1);
    let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
    assert!(encrypt(&key, "a", &big).is_err());
}

#[test]
fn written_file_has_no_signature_and_no_tmp_left() {
    let key = MediaKey::for_test(1);
    let dir = tmp("write");
    for (rel, data, sig) in [("2027/03/a.png", PNG.to_vec(), &b"\x89PNG"[..]), ("2027/03/b.wav", wav(), &b"RIFF"[..])] {
        std::fs::create_dir_all(dir.join("2027/03")).unwrap();
        write_encrypted(&key, &dir.join(rel), rel, &data).unwrap();
        let on_disk = std::fs::read(dir.join(rel)).unwrap();
        assert!(!contains(&on_disk, sig), "{rel}에 평문 시그니처가 남음");
        assert_eq!(read_plain(Some(&key), &dir.join(rel), rel).unwrap(), data);
        assert_eq!(plain_size(&dir.join(rel)), data.len() as u64);
    }
    assert!(snapshot(&dir).keys().all(|k| !k.ends_with(ENC_TMP_SUFFIX)));
    // 쓰기 실패(폴더 없음) → 오류, 임시 파일 없음
    assert!(write_encrypted(&key, &dir.join("nope/x.png"), "nope/x.png", PNG).is_err());
}

#[test]
fn open_plain_streams_plain_and_decrypts_encrypted() {
    let key = MediaKey::for_test(1);
    let dir = tmp("openplain");
    let p = put(&dir, "a.png", PNG);
    let mut out = Vec::new();
    open_plain(Some(&key), &p, "a.png").unwrap().read_to_end(&mut out).unwrap();
    assert_eq!(out, PNG);
    write_encrypted(&key, &p, "a.png", PNG).unwrap();
    out.clear();
    open_plain(Some(&key), &p, "a.png").unwrap().read_to_end(&mut out).unwrap();
    assert_eq!(out, PNG);
    assert!(open_plain(None, &p, "a.png").is_err());
    // 4바이트보다 짧은 평문
    let q = put(&dir, "t.bin", b"ab");
    out.clear();
    open_plain(None, &q, "t.bin").unwrap().read_to_end(&mut out).unwrap();
    assert_eq!(out, b"ab");
}

#[test]
fn rename_over_existing_file_replaces_it() {
    // 윈도우에서도 std::fs::rename은 대상이 있으면 바꿔 준다는 가정(전환의 마지막 단계)을 확인한다.
    let dir = tmp("rename");
    let a = put(&dir, "a.tmp", b"new");
    let b = put(&dir, "b.dat", b"old");
    std::fs::rename(&a, &b).unwrap();
    assert_eq!(std::fs::read(&b).unwrap(), b"new");
    assert!(!a.exists());
}

// ---------- 전환 ----------

fn plain_set(dir: &Path, n: usize) -> BTreeMap<String, Vec<u8>> {
    let mut m = BTreeMap::new();
    for i in 0..n {
        let rel = format!("2027/0{}/{i}-{}.{}", 1 + i % 3, new_id(), if i % 2 == 0 { "png" } else { "wav" });
        let mut data = if i % 2 == 0 { PNG.to_vec() } else { wav() };
        data.extend_from_slice(format!("#{i}").as_bytes());
        put(dir, &rel, &data);
        m.insert(rel, data);
    }
    m
}

#[test]
fn convert_dir_encrypts_every_plain_file_with_same_content() {
    let key = MediaKey::for_test(1);
    let dir = tmp("conv");
    let want = plain_set(&dir, 7);
    assert_eq!(count_state(&dir), (0, 7));
    let progress = Progress::default();
    let (n, failed) = convert_dir(&dir, &key, &RealFileOps, Some(&progress));
    assert_eq!((n, failed), (7, 0));
    assert_eq!(count_state(&dir), (7, 0));
    let (_, done, total, f) = progress.snapshot();
    assert_eq!((done, total, f), (7, 7, 0));
    for (rel, data) in &want {
        let disk = std::fs::read(dir.join(rel)).unwrap();
        assert_eq!(&disk[..4], b"TGM1");
        assert!(!contains(&disk, &data[..4]));
        assert_eq!(sha(&decrypt(&key, rel, &disk).unwrap()), sha(data), "{rel}");
    }
    assert!(snapshot(&dir).keys().all(|k| !k.ends_with(ENC_TMP_SUFFIX)));
    // 다시 돌려도 바뀌는 것이 없다
    let before = snapshot(&dir);
    assert_eq!(convert_dir(&dir, &key, &RealFileOps, None), (0, 0));
    assert_eq!(snapshot(&dir), before);
}

/// 단계별로 실패를 끼워 넣는 동작.
#[derive(Default)]
struct Faulty {
    fail_write: bool,
    /// 임시 파일을 다시 읽을 때 한 바이트를 바꿔 돌려준다(검증 불일치 흉내)
    corrupt_readback: bool,
    fail_replace: bool,
    /// 쓰기 직후 원본을 건드린다(작업 중 변경 흉내)
    touch_source_on_write: RefCell<Option<PathBuf>>,
    writes: Cell<usize>,
}

impl FileOps for Faulty {
    fn write(&self, path: &Path, bytes: &[u8]) -> DbResult<()> {
        self.writes.set(self.writes.get() + 1);
        if self.fail_write {
            // 반쯤 쓰다 실패한 흔적을 남긴다
            std::fs::write(path, &bytes[..bytes.len() / 2]).unwrap();
            return Err("주입: 쓰기 실패".to_string());
        }
        std::fs::write(path, bytes).unwrap();
        if let Some(src) = self.touch_source_on_write.borrow().as_ref() {
            let mut d = std::fs::read(src).unwrap();
            d.push(b'!');
            std::fs::write(src, d).unwrap();
        }
        Ok(())
    }
    fn read(&self, path: &Path) -> DbResult<Vec<u8>> {
        let mut d = std::fs::read(path).map_err(|e| e.to_string())?;
        if self.corrupt_readback && path.to_string_lossy().ends_with(ENC_TMP_SUFFIX) {
            let i = d.len() - 1;
            d[i] ^= 1;
        }
        Ok(d)
    }
    fn replace(&self, from: &Path, to: &Path) -> DbResult<()> {
        if self.fail_replace {
            return Err("주입: 이름 바꾸기 실패".to_string());
        }
        std::fs::rename(from, to).map_err(|e| e.to_string())
    }
}

fn assert_untouched_after(ops: Faulty, expect: &str) {
    let key = MediaKey::for_test(1);
    let dir = tmp("fault");
    let want = plain_set(&dir, 3);
    let before = snapshot(&dir);
    let rel = want.keys().next().unwrap().clone();
    if ops.touch_source_on_write.borrow().is_some() {
        *ops.touch_source_on_write.borrow_mut() = Some(dir.join(&rel));
    }
    let out = convert_file(&key, &dir.join(&rel), &rel, &ops);
    match &out {
        Outcome::Failed(why) => assert!(why.contains(expect), "{why}"),
        other => panic!("실패해야 함: {other:?}"),
    }
    let after = snapshot(&dir);
    if ops.touch_source_on_write.borrow().is_some() {
        // 원본이 바뀌었으니 바뀐 내용 그대로, 임시 파일만 없어야 한다
        assert!(after.keys().all(|k| !k.ends_with(ENC_TMP_SUFFIX)));
        let mut expected = before.clone();
        expected.get_mut(&rel).unwrap().push(b'!');
        assert_eq!(after, expected);
    } else {
        assert_eq!(after, before, "원본은 그대로이고 임시 파일도 없어야 한다");
    }
    // 폴더 전체 전환에서도 실패한 파일은 건너뛰고(원본 그대로) 계속한다
    let ops2 = Faulty { fail_write: ops.fail_write, corrupt_readback: ops.corrupt_readback, fail_replace: ops.fail_replace, ..Default::default() };
    if ops.touch_source_on_write.borrow().is_none() {
        let (n, failed) = convert_dir(&dir, &key, &ops2, None);
        assert_eq!((n, failed), (0, 3));
        assert_eq!(snapshot(&dir), before);
        assert_eq!(ops2.writes.get(), 3, "한 파일이 실패해도 다음 파일을 시도한다");
    }
}

#[test]
fn write_failure_keeps_original_and_removes_tmp() {
    assert_untouched_after(Faulty { fail_write: true, ..Default::default() }, "쓰기 실패");
}

#[test]
fn verification_mismatch_keeps_original() {
    assert_untouched_after(Faulty { corrupt_readback: true, ..Default::default() }, "복호화");
}

#[test]
fn replace_failure_keeps_original() {
    assert_untouched_after(Faulty { fail_replace: true, ..Default::default() }, "이름 바꾸기");
}

#[test]
fn source_changed_during_conversion_is_not_overwritten() {
    assert_untouched_after(Faulty { touch_source_on_write: RefCell::new(Some(PathBuf::new())), ..Default::default() }, "바뀌어");
}

#[test]
fn deleted_source_is_not_recreated() {
    struct Deleter;
    impl FileOps for Deleter {
        fn write(&self, path: &Path, bytes: &[u8]) -> DbResult<()> {
            std::fs::write(path, bytes).unwrap();
            // 첨부가 지워져 원본이 사라졌다
            std::fs::remove_file(path.with_extension("")).unwrap();
            Ok(())
        }
    }
    let key = MediaKey::for_test(1);
    let dir = tmp("deleted");
    let p = put(&dir, "a.png", PNG);
    assert!(matches!(convert_file(&key, &p, "a.png", &Deleter), Outcome::Failed(_)));
    assert!(!p.exists());
    assert!(snapshot(&dir).is_empty());
}

#[test]
fn already_encrypted_and_oversized_files_are_left_alone() {
    let key = MediaKey::for_test(1);
    let dir = tmp("skip");
    let p = put(&dir, "a.png", b"");
    write_encrypted(&key, &p, "a.png", PNG).unwrap();
    let before = std::fs::read(&p).unwrap();
    assert_eq!(convert_file(&key, &p, "a.png", &RealFileOps), Outcome::Already);
    assert_eq!(std::fs::read(&p).unwrap(), before);

    let big = dir.join("big.wav");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(MAX_FILE_BYTES + 1).unwrap();
    drop(f);
    assert!(matches!(convert_file(&key, &big, "big.wav", &RealFileOps), Outcome::Skipped(_)));
    assert_eq!(std::fs::metadata(&big).unwrap().len(), MAX_FILE_BYTES + 1);
}

#[test]
fn leftover_tmp_is_cleaned_and_conversion_continues() {
    let key = MediaKey::for_test(1);
    let dir = tmp("leftover");
    let want = plain_set(&dir, 3);
    let rel = want.keys().next().unwrap().clone();
    // 중간에 꺼진 흔적: 원본은 평문, 옆에 반쯤 쓴 tmp
    put(&dir, &format!("{rel}{ENC_TMP_SUFFIX}"), b"TGM1half");
    // 이미 끝난 파일 옆에 남은 tmp도
    put(&dir, &format!("2027/01/other.png{ENC_TMP_SUFFIX}"), b"x");
    assert_eq!(list_files(&dir).len(), 3, "tmp는 목록에서 빠진다");
    assert_eq!(cleanup_tmp(&dir), 2);
    assert!(snapshot(&dir).keys().all(|k| !k.ends_with(ENC_TMP_SUFFIX)));
    assert_eq!(snapshot(&dir), want.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    assert_eq!(convert_dir(&dir, &key, &RealFileOps, None), (3, 0));
    assert_eq!(count_state(&dir), (3, 0));
}

#[test]
fn convert_dir_stops_when_cancelled() {
    let key = MediaKey::for_test(1);
    let dir = tmp("cancel");
    plain_set(&dir, 4);
    let progress = Progress::default();
    progress.cancel();
    assert_eq!(convert_dir(&dir, &key, &RealFileOps, Some(&progress)), (0, 0));
    assert_eq!(count_state(&dir), (0, 4));
}

#[test]
fn start_converts_media_and_before_restore_folders_in_background() {
    let data = tmp("start");
    let key = MediaKey::for_test(1);
    let media = data.join("media");
    let old = data.join("before-restore-20260101-000000").join("media");
    let a = plain_set(&media, 4);
    let b = plain_set(&old, 2);
    put(&media, &format!("x.png{ENC_TMP_SUFFIX}"), b"junk");
    put(&old, &format!("y.png{ENC_TMP_SUFFIX}"), b"junk");

    let progress = std::sync::Arc::new(Progress::default());
    start(&data, Some(key.clone()), true, progress.clone());
    let t = std::time::Instant::now();
    while progress.snapshot().0 != "done" {
        assert!(t.elapsed().as_secs() < 20, "전환이 끝나지 않음");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(progress.snapshot(), ("done", 4, 4, 0));
    assert_eq!(count_state(&media), (4, 0));
    for (rel, data) in &a {
        assert_eq!(read_plain(Some(&key), &media.join(rel), rel).unwrap(), *data);
    }
    // 보관 폴더는 메인 뒤에 이어서 바뀐다
    let t = std::time::Instant::now();
    while count_state(&old) != (2, 0) {
        assert!(t.elapsed().as_secs() < 20, "보관 폴더 전환이 끝나지 않음");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for (rel, data) in &b {
        assert_eq!(read_plain(Some(&key), &old.join(rel), rel).unwrap(), *data);
    }
    assert!(snapshot(&data).keys().all(|k| !k.ends_with(ENC_TMP_SUFFIX)));
}

#[test]
fn start_without_convert_only_cleans_tmp() {
    let data = tmp("noconv");
    let media = data.join("media");
    plain_set(&media, 2);
    put(&media, &format!("x.png{ENC_TMP_SUFFIX}"), b"junk");
    let progress = std::sync::Arc::new(Progress::default());
    start(&data, Some(MediaKey::for_test(1)), false, progress.clone());
    assert_eq!(count_state(&media), (0, 2));
    assert_eq!(progress.snapshot().0, "waiting");
    assert!(snapshot(&data).keys().all(|k| !k.ends_with(ENC_TMP_SUFFIX)));
}

#[test]
fn holder_can_be_replaced_and_requires_a_key() {
    let h = MediaKeyHolder::new(None);
    assert!(h.require().is_err());
    h.set(Some(MediaKey::for_test(3)));
    let k = h.require().unwrap();
    assert_eq!(decrypt(&k, "a", &encrypt(&MediaKey::for_test(3), "a", b"z").unwrap()).unwrap(), b"z");
}

#[test]
fn load_key_reads_the_key_file() {
    let dir = tmp("loadkey");
    assert!(load_key(&dir).is_none());
    let k = DbKey::generate().unwrap();
    key::write_key_file(&dir, &k).unwrap();
    let m = load_key(&dir).unwrap();
    assert_eq!(decrypt(&m, "a", &encrypt(&MediaKey::from_db_key(&k), "a", b"q").unwrap()).unwrap(), b"q");
}
