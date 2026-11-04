use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use age::{scrypt, Encryptor, Recipient};
use rusqlite::Connection;

use super::format::{create, inspect, open_stream};
use super::restore::{prune_before_restore, restore, swap_data, swap_data_with};
use super::*;
use crate::db::observations::tests::{input, media as unused_media};
use crate::db::{attachments, classes, new_id, observations, open, students};

const PW: &str = "correct horse";
const WF: Option<u8> = Some(2);

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tonghap-backup-{}-{}", name, new_id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 학급 1, 학생 3, 관찰 1, 첨부 2개가 든 data 폴더.
fn seeded(name: &str) -> (PathBuf, Mutex<Connection>) {
    let dir = tmp(name).join("data");
    let (mut conn, media) = open(&dir).unwrap();
    let c = classes::create(&conn, 2027, 1, "1반").unwrap();
    students::import(&mut conn, &c.id, "1 가\n2 나\n3 다").unwrap();
    let ids: Vec<String> = students::list(&conn, &c.id, true).unwrap().into_iter().map(|s| s.id).collect();
    let o = observations::create(&mut conn, &unused_media(), &input(&c.id, &[&ids[0]])).unwrap();
    attachments::add_bytes(&conn, &media.0, &o.id, "audio", "audio/webm", b"first-audio-bytes").unwrap();
    attachments::add_bytes(&conn, &media.0, &o.id, "audio", "audio/ogg", &[7u8; 5000]).unwrap();
    (dir, Mutex::new(conn))
}

fn counts(db: &Mutex<Connection>) -> Counts {
    let conn = db.lock().unwrap();
    let n = |t: &str| -> i64 { conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap() };
    Counts { classes: n("classes"), students: n("students"), observations: n("observations"), attachments: n("attachments") }
}

fn attachment_files(db: &Mutex<Connection>, data: &Path) -> Vec<(String, Vec<u8>)> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT rel_path FROM attachments ORDER BY rel_path").unwrap();
    let rels: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    rels.into_iter().map(|r| (r.clone(), std::fs::read(attachments::abs_path(&data.join("media"), &r)).unwrap())).collect()
}

fn make_backup(db: &Mutex<Connection>, data: &Path) -> PathBuf {
    let out = data.parent().unwrap().join(format!("{}.tonghap", new_id()));
    create(db, data, &out, PW, WF, &mut |_| {}).unwrap();
    out
}

fn run_restore(db: &Mutex<Connection>, data: &Path, file: &Path, pw: &str) -> DbResult<BackupSummary> {
    restore(db, data, file, pw, &mut |_| {})
}

/// 만든 백업을 풀어 (매니페스트, 항목들)로 돌려준다.
fn unpack(file: &Path) -> (Manifest, Vec<(String, Vec<u8>)>) {
    let mut ar = tar::Archive::new(open_stream(file, PW).unwrap());
    let mut manifest = None;
    let mut files = Vec::new();
    for e in ar.entries().unwrap() {
        let mut e = e.unwrap();
        let name = String::from_utf8(e.path_bytes().into_owned()).unwrap();
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).unwrap();
        if name == MANIFEST {
            manifest = Some(serde_json::from_slice(&buf).unwrap());
        } else {
            files.push((name, buf));
        }
    }
    (manifest.unwrap(), files)
}

/// 이름을 검사 없이 그대로 쓰는 tar를 age로 감싸 쓴다.
fn craft(out: &Path, manifest: &Manifest, files: &[(String, Vec<u8>)]) {
    let mut r = scrypt::Recipient::new(secret(PW));
    r.set_work_factor(2);
    let enc = Encryptor::with_recipients(std::iter::once(&r as &dyn Recipient)).unwrap();
    let w = enc.wrap_output(std::fs::File::create(out).unwrap()).unwrap();
    let mut tar = tar::Builder::new(w);
    let m = serde_json::to_vec(manifest).unwrap();
    let all = std::iter::once((MANIFEST.to_string(), m)).chain(files.iter().cloned());
    for (name, data) in all {
        let mut h = tar::Header::new_gnu();
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        tar.append(&h, &data[..]).unwrap();
    }
    tar.finish().unwrap();
    tar.into_inner().unwrap().finish().unwrap();
}

fn tampered(mutate: impl FnOnce(&mut Manifest, &mut Vec<(String, Vec<u8>)>)) -> (PathBuf, PathBuf, Mutex<Connection>, Counts) {
    let (data, db) = seeded("src");
    let file = make_backup(&db, &data);
    let (mut m, mut files) = unpack(&file);
    mutate(&mut m, &mut files);
    let bad = data.parent().unwrap().join("bad.tonghap");
    craft(&bad, &m, &files);
    // 되돌릴 대상 데이터: 별도 폴더
    let (target, target_db) = seeded("target");
    let before = counts(&target_db);
    (bad, target, target_db, before)
}

fn assert_untouched(target: &Path, db: &Mutex<Connection>, before: Counts) {
    assert_eq!(counts(db), before);
    assert!(!target.join(restore::STAGING).exists());
    assert!(!std::fs::read_dir(target).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("before-restore-")));
}

#[test]
fn backup_then_restore_into_other_data_dir_keeps_counts_and_bytes() {
    let (src, src_db) = seeded("src");
    let file = make_backup(&src_db, &src);
    assert!(!file.with_extension("tonghap.part").exists());

    let sum = inspect(&file, PW).unwrap();
    assert_eq!(sum.counts, Counts { classes: 1, students: 3, observations: 1, attachments: 2 });
    assert_eq!(sum.file_count, 3);
    assert_eq!(sum.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(sum.schema_version, crate::db::migrations::latest_version());

    let dst = tmp("dst").join("data");
    let (conn, _) = open(&dst).unwrap();
    let dst_db = Mutex::new(conn);
    let mut stages = Vec::new();
    let got = restore(&dst_db, &dst, &file, PW, &mut |p| stages.push(p.stage)).unwrap();
    assert_eq!(got, sum);
    for s in ["reading", "verifying", "swapping"] {
        assert!(stages.contains(&s), "{s}");
    }
    assert_eq!(counts(&dst_db), counts(&src_db));
    let a = attachment_files(&src_db, &src);
    assert_eq!(a.len(), 2);
    assert_eq!(a, attachment_files(&dst_db, &dst));
    // 복원된 연결은 쓸 수 있고 외래 키가 켜져 있다.
    let fk: i64 = dst_db.lock().unwrap().query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(fk, 1);
    assert!(!dst.join(restore::STAGING).exists());
    // 백업 후 lastBackup 설정이 저장됨(복원본은 백업 시점 이전 스냅숏이므로 원본에서 확인)
    let st = status(&src_db.lock().unwrap()).unwrap();
    assert_eq!(st.last_backup.unwrap().observations, 1);
    assert_eq!(st.observations_since, 0);
}

#[test]
fn password_rules_and_missing_attachment_is_reported() {
    let (src, db) = seeded("pw");
    let out = src.parent().unwrap().join("x.tonghap");
    for bad in ["", "        ", "short"] {
        assert!(create(&db, &src, &out, bad, WF, &mut |_| {}).is_err(), "{bad:?}");
    }
    assert!(!out.exists());
    let rel = attachment_files(&db, &src).remove(0).0;
    std::fs::remove_file(attachments::abs_path(&src.join("media"), &rel)).unwrap();
    // DB에 없는 디스크 파일은 넣지 않는다.
    std::fs::write(src.join("media").join("stray.bin"), b"x").unwrap();
    let r = create(&db, &src, &out, PW, WF, &mut |_| {}).unwrap();
    assert_eq!(r.missing, [rel]);
    let (m, files) = unpack(&out);
    assert_eq!(m.missing.len(), 1);
    assert_eq!(files.len(), 2); // db + 첨부 1개
    assert!(!files.iter().any(|(n, _)| n.contains("stray")));
    assert!(!src.join(".backup-tmp").exists());
}

#[test]
fn wrong_password_and_non_backup_files() {
    let (src, db) = seeded("wrong");
    let file = make_backup(&db, &src);
    assert_eq!(inspect(&file, "another password").unwrap_err(), ERR_PASSWORD);
    let dir = tmp("junk");
    let random = dir.join("random.tonghap");
    std::fs::write(&random, (0..4000u32).map(|i| (i * 31 % 251) as u8).collect::<Vec<_>>()).unwrap();
    assert_eq!(inspect(&random, PW).unwrap_err(), ERR_NOT_BACKUP);
    let empty = dir.join("empty.tonghap");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(inspect(&empty, PW).unwrap_err(), ERR_NOT_BACKUP);
    // 틀린 암호로 복원해도 현재 데이터는 그대로
    let (target, tdb) = seeded("t");
    let before = counts(&tdb);
    assert_eq!(run_restore(&tdb, &target, &file, "nope nope nope").unwrap_err(), ERR_PASSWORD);
    assert_untouched(&target, &tdb, before);
}

#[test]
fn tampered_ciphertext_is_rejected_and_data_unchanged() {
    let (src, db) = seeded("tamper");
    let file = make_backup(&db, &src);
    let mut bytes = std::fs::read(&file).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x55;
    let bad = src.parent().unwrap().join("tampered.tonghap");
    std::fs::write(&bad, &bytes).unwrap();
    let (target, tdb) = seeded("t");
    let before = counts(&tdb);
    let e = run_restore(&tdb, &target, &bad, PW).unwrap_err();
    assert!(e == ERR_CORRUPT || e.contains("올바르지 않습니다"), "{e}");
    assert_untouched(&target, &tdb, before);
    // 끝이 잘린 파일도 거부
    let cut = src.parent().unwrap().join("cut.tonghap");
    std::fs::write(&cut, &std::fs::read(&file).unwrap()[..bytes.len() - 20]).unwrap();
    assert!(run_restore(&tdb, &target, &cut, PW).is_err());
    assert_untouched(&target, &tdb, before);
}

#[test]
fn rejects_dotdot_absolute_and_unlisted_entries() {
    // 매니페스트에 없는 파일
    let (bad, target, tdb, before) = tampered(|_, f| f.push(("media/2027/03/extra.webm".into(), b"x".to_vec())));
    assert!(run_restore(&tdb, &target, &bad, PW).unwrap_err().contains("목록에 없는 파일"));
    assert_untouched(&target, &tdb, before);
    // .. 경로 (매니페스트에도 적어 두어도 거부)
    let (bad, target, tdb, before) = tampered(|m, f| {
        f.push(("media/../../evil.txt".into(), b"x".to_vec()));
        m.files.push(FileEntry { path: "media/../../evil.txt".into(), bytes: 1, sha256: "00".into() });
    });
    assert!(run_restore(&tdb, &target, &bad, PW).is_err());
    assert!(!target.parent().unwrap().join("evil.txt").exists());
    assert_untouched(&target, &tdb, before);
    // 절대 경로, 허용되지 않는 최상위 이름
    for name in ["/tmp/evil.txt", "other/file.txt", "media"] {
        let (bad, target, tdb, before) = tampered(|_, f| f.push((name.into(), b"x".to_vec())));
        assert!(run_restore(&tdb, &target, &bad, PW).is_err(), "{name}");
        assert_untouched(&target, &tdb, before);
    }
    // 매니페스트에 있으나 tar에 없는 파일
    let (bad, target, tdb, before) = tampered(|_, f| {
        f.pop();
    });
    assert!(run_restore(&tdb, &target, &bad, PW).unwrap_err().contains("빠진 파일"));
    assert_untouched(&target, &tdb, before);
}

#[test]
fn rejects_symlink_entries() {
    let (data, db) = seeded("sym");
    let file = make_backup(&db, &data);
    let (m, files) = unpack(&file);
    let bad = data.parent().unwrap().join("sym.tonghap");
    let mut r = scrypt::Recipient::new(secret(PW));
    r.set_work_factor(2);
    let enc = Encryptor::with_recipients(std::iter::once(&r as &dyn Recipient)).unwrap();
    let mut tar = tar::Builder::new(enc.wrap_output(std::fs::File::create(&bad).unwrap()).unwrap());
    let mj = serde_json::to_vec(&m).unwrap();
    let mut h = tar::Header::new_gnu();
    h.set_size(mj.len() as u64);
    h.set_cksum();
    tar.append_data(&mut h, MANIFEST, &mj[..]).unwrap();
    let mut l = tar::Header::new_gnu();
    l.set_entry_type(tar::EntryType::Symlink);
    l.set_size(0);
    l.set_link_name("/etc/passwd").unwrap();
    tar.append_data(&mut l, "media/link", &b""[..]).unwrap();
    for (n, d) in &files {
        let mut h = tar::Header::new_gnu();
        h.set_size(d.len() as u64);
        tar.append_data(&mut h, n, &d[..]).unwrap();
    }
    tar.finish().unwrap();
    tar.into_inner().unwrap().finish().unwrap();
    let (target, tdb) = seeded("t");
    let before = counts(&tdb);
    assert!(run_restore(&tdb, &target, &bad, PW).unwrap_err().contains("허용되지 않는 항목"));
    assert_untouched(&target, &tdb, before);
}

#[test]
fn rejects_hash_mismatch() {
    let (bad, target, tdb, before) = tampered(|_, f| {
        let last = f.last_mut().unwrap();
        last.1[0] ^= 1;
    });
    assert!(run_restore(&tdb, &target, &bad, PW).unwrap_err().contains("해시"));
    assert_untouched(&target, &tdb, before);
}

#[test]
fn rejects_newer_schema_and_format() {
    let (bad, target, tdb, before) = tampered(|m, _| m.schema_version = 99);
    assert_eq!(inspect(&bad, PW).unwrap_err(), ERR_NEWER);
    assert_eq!(run_restore(&tdb, &target, &bad, PW).unwrap_err(), ERR_NEWER);
    assert_untouched(&target, &tdb, before);
    let (bad, ..) = tampered(|m, _| m.format = "other".into());
    assert_eq!(inspect(&bad, PW).unwrap_err(), ERR_NOT_BACKUP);
}

#[test]
fn restore_keeps_previous_data_and_only_three_before_restore_folders() {
    let (src, src_db) = seeded("src");
    let file = make_backup(&src_db, &src);
    let (target, tdb) = seeded("t");
    let old_files = attachment_files(&tdb, &target);
    let old_counts = counts(&tdb);

    run_restore(&tdb, &target, &file, PW).unwrap();
    let list = |dir: &Path| -> Vec<PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("before-restore-"))
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    };
    let first = list(&target);
    assert_eq!(first.len(), 1);
    let old = &first[0];
    assert!(old.join("tonghap.db").is_file());
    for (rel, bytes) in &old_files {
        assert_eq!(&std::fs::read(attachments::abs_path(&old.join("media"), rel)).unwrap(), bytes);
    }
    // 보관된 이전 DB는 암호화된 채로 옮겨지며 같은 키로 열린다.
    let kept = crypt::open_keyed(&old.join("tonghap.db"), &key_of(&target)).unwrap();
    let n: i64 = kept.query_row("SELECT count(*) FROM students", [], |r| r.get(0)).unwrap();
    assert_eq!(n, old_counts.students);
    // 윈도우는 열린 파일이 든 폴더를 지울 수 없으므로 정리 전에 닫는다.
    drop(kept);

    for _ in 0..3 {
        run_restore(&tdb, &target, &file, PW).unwrap();
    }
    let after = list(&target);
    assert_eq!(after.len(), 3);
    assert!(!after.contains(old), "가장 오래된 폴더가 지워져야 함");
    assert_eq!(counts(&tdb), counts(&src_db));
}

#[test]
fn prune_keeps_newest() {
    let d = tmp("prune");
    for n in ["a", "b", "c", "d", "e"] {
        std::fs::create_dir_all(d.join(format!("before-restore-2027010{}-000000", n.len() + n.as_bytes()[0] as usize))).unwrap();
    }
    std::fs::create_dir_all(d.join("media")).unwrap();
    prune_before_restore(&d, 3);
    let n = std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("before-restore-")).count();
    assert_eq!(n, 3);
    assert!(d.join("media").exists());
}

#[test]
fn swap_failure_rolls_back_to_original_data() {
    let (target, tdb) = seeded("t");
    let before = counts(&tdb);
    let files = attachment_files(&tdb, &target);
    let staging = target.join(restore::STAGING);
    std::fs::create_dir_all(staging.join("media")).unwrap(); // tonghap.db가 없어 교체가 중간에 실패
    let err = swap_data(&tdb, &target, &staging, "20270101-000000").unwrap_err();
    assert!(err.contains("되돌렸습니다"), "{err}");
    assert_eq!(counts(&tdb), before);
    assert_eq!(attachment_files(&tdb, &target), files);
    assert!(!target.join("before-restore-20270101-000000").exists());
    // 되돌린 연결로 계속 쓸 수 있다.
    tdb.lock().unwrap().execute("INSERT INTO settings (key, value) VALUES ('k', '1')", []).unwrap();
}

#[test]
fn status_counts_new_observations_since_last_backup() {
    let (src, db) = seeded("status");
    let st = status(&db.lock().unwrap()).unwrap();
    assert!(st.last_backup.is_none());
    assert_eq!(st.observations_since, 1);
    make_backup(&db, &src);
    assert_eq!(status(&db.lock().unwrap()).unwrap().observations_since, 0);
    {
        let mut conn = db.lock().unwrap();
        let cid: String = conn.query_row("SELECT id FROM classes", [], |r| r.get(0)).unwrap();
        let sid: String = conn.query_row("SELECT id FROM students LIMIT 1", [], |r| r.get(0)).unwrap();
        observations::create(&mut conn, &unused_media(), &input(&cid, &[&sid])).unwrap();
        // 백업 시각 이후로 보이도록 설정을 과거로 돌린다.
        settings::set(&conn, "lastBackup", &serde_json::json!({"at": "2000-01-01T00:00:00+09:00", "path": "x", "observations": 1})).unwrap();
    }
    assert_eq!(status(&db.lock().unwrap()).unwrap().observations_since, 2);
}

/// 원래 파일을 옮기는 도중 실패하면(예: 윈도우에서 백신이 파일을 잡고 있음),
/// 아직 옮기지 못한 원래 파일이 지워지면 안 된다.
#[test]
fn failed_move_of_original_never_deletes_it() {
    for fail_on in ["tonghap.db", "media"] {
        let (target, tdb) = seeded(&format!("lock-{fail_on}"));
        let before = counts(&tdb);
        let files = attachment_files(&tdb, &target);
        let staging = target.join(restore::STAGING);
        std::fs::create_dir_all(staging.join("media")).unwrap();
        std::fs::write(staging.join("tonghap.db"), b"").unwrap();
        let data = target.clone();
        let failing = move |from: &Path, to: &Path| -> crate::db::DbResult<()> {
            if from == data.join(fail_on) {
                return Err(format!("잠김: {}", from.display()));
            }
            std::fs::rename(from, to).map_err(|e| e.to_string())
        };
        let err = swap_data_with(&tdb, &target, &staging, "20270101-000000", &failing).unwrap_err();
        assert!(err.contains("되돌렸습니다"), "{err}");
        assert_eq!(counts(&tdb), before, "fail_on={fail_on}");
        assert_eq!(attachment_files(&tdb, &target), files, "fail_on={fail_on}");
    }
}

#[test]
fn restoring_a_version_1_backup_migrates_it_to_the_latest_schema() {
    // 버전 1 스키마만 적용한 DB로 백업을 만든다.
    let src = tmp("v1").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v1 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v1, 1).unwrap();
    v1.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at)
           VALUES ('o1', 'c1', '2027-03-05T10:00:00+09:00', 'g1s1-school', '옛 기록', 't', 't');",
    )
    .unwrap();
    let src_db = Mutex::new(v1);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 1);

    let (target, tdb) = seeded("v1-target");
    let sum = run_restore(&tdb, &target, &file, PW).unwrap();
    assert_eq!(sum.schema_version, 1);

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let (note, link): (String, Option<String>) =
        conn.query_row("SELECT note, unit_session_id FROM observations WHERE id = 'o1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((note.as_str(), link), ("옛 기록", None));
    let units: i64 = conn.query_row("SELECT count(*) FROM units", [], |r| r.get(0)).unwrap();
    assert_eq!(units, 0);
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(fk, 1);
}

#[test]
fn restoring_a_version_2_backup_migrates_it_to_the_board_schema() {
    let src = tmp("v2").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v2 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v2, 2).unwrap();
    v2.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO units (id, class_id, tool_id, title, created_at, updated_at) VALUES ('u1', 'c1', 'free', '옛 단원', 't', 't');",
    )
    .unwrap();
    let src_db = Mutex::new(v2);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 2);

    let (target, tdb) = seeded("v2-target");
    run_restore(&tdb, &target, &file, PW).unwrap();

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let title: String = conn.query_row("SELECT title FROM units WHERE id = 'u1'", [], |r| r.get(0)).unwrap();
    assert_eq!(title, "옛 단원");
    for t in ["custom_plays", "play_log", "habits", "habit_checks"] {
        let n: i64 = conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{t}");
    }
}

#[test]
fn restoring_a_version_8_backup_adds_the_combined_evaluation_table_and_keeps_drafts() {
    let src = tmp("v8").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v8 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v8, 8).unwrap();
    v8.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s1', 'c1', 1, '김하늘', 't');
         INSERT INTO evaluation_drafts (id, class_id, student_id, subject, period_from, period_to, sentences, created_at, updated_at)
           VALUES ('d1', 'c1', 's1', '슬', '2027-03-02', '2027-07-18', '[]', 't', 't');",
    )
    .unwrap();
    let src_db = Mutex::new(v8);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 8);

    let (target, tdb) = seeded("v8-target");
    let sum = run_restore(&tdb, &target, &file, PW).unwrap();
    assert_eq!(sum.schema_version, 8);

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let drafts: i64 = conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
    assert_eq!(drafts, 1);
    let combined: i64 = conn.query_row("SELECT count(*) FROM evaluation_combined", [], |r| r.get(0)).unwrap();
    assert_eq!(combined, 0);
}

#[test]
fn restoring_a_version_3_backup_migrates_attachments_to_the_transcript_schema() {
    let src = tmp("v3").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v3 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v3, 3).unwrap();
    v3.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at)
           VALUES ('o1', 'c1', '2027-03-05T10:00:00+09:00', 'g1s1-school', '옛 기록', 't', 't');
         INSERT INTO attachments (id, observation_id, kind, rel_path, mime, bytes, transcript, created_at)
           VALUES ('a1', 'o1', 'audio', '2027/03/a1.webm', 'audio/webm', 3, '옛 자막', 't');",
    )
    .unwrap();
    std::fs::create_dir_all(src.join("media/2027/03")).unwrap();
    std::fs::write(src.join("media/2027/03/a1.webm"), b"abc").unwrap();
    let src_db = Mutex::new(v3);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 3);

    let (target, tdb) = seeded("v3-target");
    run_restore(&tdb, &target, &file, PW).unwrap();

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let (text, status, edited): (String, String, i64) = conn
        .query_row("SELECT transcript, transcript_status, transcript_edited FROM attachments WHERE id = 'a1'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((text.as_str(), status.as_str(), edited), ("옛 자막", "none", 0));
}

#[test]
fn restoring_a_version_4_backup_migrates_it_to_the_parent_report_schema() {
    let src = tmp("v4").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v4 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v4, 4).unwrap();
    v4.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s1', 'c1', 1, '김하늘', 't');
         INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at)
           VALUES ('o1', 'c1', '2027-03-05T10:00:00+09:00', 'g1s1-school', '옛 기록', 't', 't');
         INSERT INTO observation_students (observation_id, student_id) VALUES ('o1', 's1');",
    )
    .unwrap();
    let src_db = Mutex::new(v4);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 4);

    let (target, tdb) = seeded("v4-target");
    run_restore(&tdb, &target, &file, PW).unwrap();

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let note: String = conn.query_row("SELECT note FROM observations WHERE id = 'o1'", [], |r| r.get(0)).unwrap();
    assert_eq!(note, "옛 기록");
    let reports: i64 = conn.query_row("SELECT count(*) FROM parent_reports", [], |r| r.get(0)).unwrap();
    assert_eq!(reports, 0);
}

#[test]
fn restoring_a_version_5_backup_migrates_it_to_the_evaluation_schema() {
    let src = tmp("v5").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v5 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v5, 5).unwrap();
    v5.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s1', 'c1', 1, '김하늘', 't');
         INSERT INTO parent_reports (id, class_id, student_id, period_from, period_to, message, created_at, updated_at)
           VALUES ('r1', 'c1', 's1', '2027-03-01', '2027-04-01', '옛 리포트', 't', 't');",
    )
    .unwrap();
    let src_db = Mutex::new(v5);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 5);

    let (target, tdb) = seeded("v5-target");
    run_restore(&tdb, &target, &file, PW).unwrap();

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let message: String = conn.query_row("SELECT message FROM parent_reports WHERE id = 'r1'", [], |r| r.get(0)).unwrap();
    assert_eq!(message, "옛 리포트");
    let drafts: i64 = conn.query_row("SELECT count(*) FROM evaluation_drafts", [], |r| r.get(0)).unwrap();
    assert_eq!(drafts, 0);
}

#[test]
fn restoring_a_version_6_backup_migrates_it_to_the_activity_schema() {
    let src = tmp("v6").join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let v6 = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&v6, 6).unwrap();
    v6.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO units (id, class_id, tool_id, title, created_at, updated_at) VALUES ('u1', 'c1', 'free', '옛 단원', 't', 't');
         INSERT INTO unit_stages (id, unit_id, position, title) VALUES ('g1', 'u1', 0, '수업');
         INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, created_at, updated_at)
           VALUES ('s1', 'u1', 'g1', 0, '옛 차시', 't', 't');",
    )
    .unwrap();
    let src_db = Mutex::new(v6);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, 6);

    let (target, tdb) = seeded("v6-target");
    run_restore(&tdb, &target, &file, PW).unwrap();

    let conn = tdb.lock().unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::db::migrations::latest_version());
    let (title, activity): (String, Option<String>) =
        conn.query_row("SELECT title, activity_id FROM unit_sessions WHERE id = 's1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((title.as_str(), activity), ("옛 차시", None));
    let custom: i64 = conn.query_row("SELECT count(*) FROM custom_activities", [], |r| r.get(0)).unwrap();
    assert_eq!(custom, 0);
}

#[test]
fn throttle_thins_progress_but_keeps_stage_changes_and_ends() {
    use std::time::{Duration, Instant};
    let mut t = Throttle::new(PROGRESS_MIN_INTERVAL);
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    assert!(t.pass_at(&progress("hashing", 1, 100), at(0)), "첫 이벤트는 통과");
    assert!(!t.pass_at(&progress("hashing", 2, 100), at(10)));
    assert!(!t.pass_at(&progress("hashing", 3, 100), at(99)));
    assert!(t.pass_at(&progress("hashing", 4, 100), at(100)), "100ms가 지나면 통과");
    assert!(!t.pass_at(&progress("hashing", 5, 100), at(150)));
    assert!(t.pass_at(&progress("hashing", 100, 100), at(151)), "단계가 끝나는 이벤트는 항상 통과");
    assert!(t.pass_at(&progress("writing", 0, 50), at(152)), "단계가 바뀌면 통과");
    // 1,500개 파일이 몇 ms 안에 쏟아져도 이벤트는 몇 개만 남는다.
    let mut passed = 0;
    for i in 0..1500u64 {
        if t.pass_at(&progress("writing", i, 5000), at(200 + i / 10)) {
            passed += 1;
        }
    }
    assert!(passed <= 3, "통과 {passed}개");
}

// ---------- DB 암호화(설계 20) ----------

use crate::db::crypt::{self, FileKind};
use crate::db::key::{self, DbKey, KeyFile};
use crate::db::DbState;

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

fn key_of(data: &Path) -> DbKey {
    match key::read_key_file(data) {
        KeyFile::Key(k) => k,
        other => panic!("{other:?}"),
    }
}

/// `data/tonghap.db`가 암호화되어 있고 현재 키로 열리며, 평문 DB가 어디에도 없다.
fn assert_encrypted(data: &Path) {
    assert_eq!(crypt::file_kind(&data.join("tonghap.db")).unwrap(), FileKind::Encrypted);
    assert!(crypt::open_keyed(&data.join("tonghap.db"), &key_of(data)).is_ok());
    assert!(!data.join(restore::STAGING).exists());
    assert!(!data.join(".backup-tmp").exists());
    assert!(!data.join("tonghap.db.enc-tmp").exists() && !data.join("tonghap.db.plain-old").exists());
}

#[test]
fn backup_vault_keeps_a_plain_sqlite_db_and_leaves_no_temp_files() {
    let (src, src_db) = seeded("enc-vault");
    assert_encrypted(&src);
    let file = make_backup(&src_db, &src);
    assert!(!src.join(".backup-tmp").exists(), "평문 임시 복사본이 남았음");
    let (m, files) = unpack(&file);
    let db = &files.iter().find(|(n, _)| n == "tonghap.db").unwrap().1;
    assert_eq!(&db[..16], SQLITE_MAGIC, "보관함 안 DB는 평문 SQLite(백업 형식 유지)");
    assert_eq!(m.counts, Counts { classes: 1, students: 3, observations: 1, attachments: 2 });
    // 예전 앱(키 없이 SQLite로 여는 쪽)이 그대로 열 수 있다.
    let probe = tmp("enc-vault-probe").join("x.db");
    std::fs::write(&probe, db).unwrap();
    let c = Connection::open(&probe).unwrap();
    let n: i64 = c.query_row("SELECT count(*) FROM students", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3);
    // 백업 파일 어디에도 평문 SQLite 머리가 겉으로 보이지 않는다(보관함 전체가 암호화).
    assert!(!std::fs::read(&file).unwrap().windows(16).any(|w| w == SQLITE_MAGIC));
}

#[test]
fn backup_failure_leaves_no_plain_copy() {
    let (src, src_db) = seeded("enc-fail");
    let out = src.parent().unwrap().join("nope").join("x.tonghap"); // 폴더가 없어 쓰기 단계에서 실패
    let err = create(&src_db, &src, &out, PW, WF, &mut |_| {}).unwrap_err();
    assert!(!err.is_empty());
    assert!(!src.join(".backup-tmp").exists());
    // 앱 연결은 그대로 쓸 수 있다.
    src_db.lock().unwrap().execute("INSERT INTO settings (key, value) VALUES ('k', '1')", []).unwrap();
}

#[test]
fn restore_into_encrypted_database_round_trips_and_is_encrypted() {
    let (src, src_db) = seeded("enc-src");
    let file = make_backup(&src_db, &src);
    let (dst, dst_db) = seeded("enc-dst");
    let key_before = std::fs::read(dst.join("db.key")).unwrap();
    run_restore(&dst_db, &dst, &file, PW).unwrap();
    assert_eq!(counts(&dst_db), counts(&src_db));
    assert_encrypted(&dst);
    assert_eq!(std::fs::read(dst.join("db.key")).unwrap(), key_before, "키는 바뀌지 않는다");
    // 보관 폴더로 옮겨진 이전 DB도 암호화되어 있다.
    let kept: Vec<_> = std::fs::read_dir(&dst).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("before-restore-")).collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(crypt::file_kind(&kept[0].path().join("tonghap.db")).unwrap(), FileKind::Encrypted);
    // 다시 열어도 같다.
    drop(dst_db);
    let (conn, _) = open(&dst).unwrap();
    let n: i64 = conn.query_row("SELECT count(*) FROM students", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3);
}

/// 예전 앱이 만든 평문 DB(스키마 `version`)로 백업을 만들어 새 data에 복원한다.
fn restore_old_plain_backup(version: i64) -> (PathBuf, Mutex<Connection>) {
    let src = tmp(&format!("old-v{version}")).join("data");
    std::fs::create_dir_all(src.join("media")).unwrap();
    let old = Connection::open(src.join("tonghap.db")).unwrap();
    crate::db::migrations::apply_up_to(&old, version).unwrap();
    old.execute_batch(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c1', 2027, 1, '1반', 't');
         INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s1', 'c1', 1, '김하늘', 't');",
    )
    .unwrap();
    let src_db = Mutex::new(old);
    let file = make_backup(&src_db, &src);
    assert_eq!(inspect(&file, PW).unwrap().schema_version, version);
    let (target, tdb) = seeded(&format!("old-v{version}-target"));
    run_restore(&tdb, &target, &file, PW).unwrap();
    (target, tdb)
}

#[test]
fn restoring_old_plain_v8_and_v9_backups_results_in_an_encrypted_database() {
    for v in [8, 9] {
        let (target, tdb) = restore_old_plain_backup(v);
        assert_encrypted(&target);
        let c = tdb.lock().unwrap();
        let name: String = c.query_row("SELECT name FROM students WHERE id = 's1'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "김하늘", "v{v}");
        let uv: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(uv, crate::db::migrations::latest_version());
        drop(c);
        // 파일 어디에도 이름이 평문으로 없다.
        for e in std::fs::read_dir(&target).unwrap().flatten() {
            if e.path().is_file() && e.file_name() != "db.key" {
                assert!(!std::fs::read(e.path()).unwrap().windows("김하늘".len()).any(|w| w == "김하늘".as_bytes()), "{:?}", e.path());
            }
        }
    }
}

/// 잠긴 상태의 data(암호화 DB는 있으나 키를 쓸 수 없음)를 만든다. `break_key`가 키 파일을 망가뜨린다.
fn locked_dir(name: &str, break_key: impl FnOnce(&Path)) -> (PathBuf, Mutex<Connection>) {
    let (dir, db) = seeded(name);
    drop(db);
    break_key(&dir);
    let o = crate::db::open_full(&dir).unwrap();
    assert!(matches!(o.state, DbState::Locked(_)), "{:?}", o.state);
    (dir, Mutex::new(o.conn))
}

fn dir_names(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(prefix)).map(|e| e.path()).collect();
    v.sort();
    v
}

#[test]
fn restoring_from_a_locked_state_keeps_the_unreadable_files_and_creates_a_new_key() {
    let (src, src_db) = seeded("lk-src");
    let file = make_backup(&src_db, &src);
    for how in ["missing", "junk", "wrong"] {
        let (dir, db) = locked_dir(&format!("lk-{how}"), |d| match how {
            "missing" => std::fs::remove_file(d.join("db.key")).unwrap(),
            "junk" => std::fs::write(d.join("db.key"), b"junk").unwrap(),
            _ => key::write_key_file(d, &DbKey::generate().unwrap()).unwrap(),
        });
        let old_db = std::fs::read(dir.join("tonghap.db")).unwrap();
        let old_key = std::fs::read(dir.join("db.key")).ok();

        run_restore(&db, &dir, &file, PW).unwrap();

        assert_eq!(counts(&db), counts(&src_db), "{how}");
        assert_encrypted(&dir);
        let unreadable = dir_names(&dir, "unreadable-");
        assert_eq!(unreadable.len(), 1, "{how}");
        assert!(dir_names(&dir, "before-restore-").is_empty(), "{how}: 열 수 없던 파일은 before-restore가 아니라 unreadable 폴더로");
        assert_eq!(std::fs::read(unreadable[0].join("tonghap.db")).unwrap(), old_db, "{how}: 열 수 없던 DB가 그대로 보관되어야 함");
        if how == "junk" {
            assert_eq!(std::fs::read(unreadable[0].join("db.key")).unwrap(), old_key.unwrap(), "풀 수 없던 키 파일도 함께 보관");
        }
        if how == "missing" {
            assert!(!unreadable[0].join("db.key").exists());
        }
        // 다시 열면 열린다.
        drop(db);
        let o = crate::db::open_full(&dir).unwrap();
        assert_eq!(o.state, DbState::Ok, "{how}");
        // unreadable 폴더는 3개 제한(prune) 대상이 아니다.
        let (_dir2, db2) = (dir.clone(), Mutex::new(o.conn));
        for _ in 0..4 {
            run_restore(&db2, &dir, &file, PW).unwrap();
        }
        assert_eq!(dir_names(&dir, "unreadable-").len(), 1);
        assert_eq!(dir_names(&dir, "before-restore-").len(), 3);
    }
}

#[test]
fn failed_swap_in_a_locked_state_restores_the_old_files_and_key() {
    let (dir, db) = locked_dir("lk-swap", |d| std::fs::write(d.join("db.key"), b"junk").unwrap());
    let before = snapshot_dir(&dir);
    let (src, src_db) = seeded("lk-swap-src");
    let file = make_backup(&src_db, &src);
    let staging = dir.join(restore::STAGING);
    restore::extract_and_verify(&file, PW, &staging, &mut |_| {}).unwrap();
    let k = DbKey::generate().unwrap();
    crypt::encrypt_file_in_place(&staging.join("tonghap.db"), &k).unwrap();
    key::write_key_file_at(&staging.join("db.key"), &k).unwrap();
    // 새 db가 자리에 놓인 뒤 매체 폴더 이동에서 실패한다.
    let fail = |from: &Path, to: &Path| -> DbResult<()> {
        if from.ends_with("media") && from.starts_with(&staging) {
            return Err("주입".to_string());
        }
        std::fs::rename(from, to).map_err(|e| e.to_string())
    };
    std::fs::create_dir_all(staging.join("media")).unwrap();
    let err = swap_data_with(&db, &dir, &staging, "20270101-000000", &fail).unwrap_err();
    assert!(err.contains("되돌렸습니다"), "{err}");
    let _ = std::fs::remove_dir_all(&staging);
    assert_eq!(snapshot_dir(&dir), before, "열 수 없던 파일과 키 파일이 그대로 돌아와야 함");
    assert!(dir_names(&dir, "unreadable-").is_empty());
}

fn snapshot_dir(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn locked_backup_attempt_is_not_possible_without_a_real_database_but_restore_with_wrong_password_changes_nothing() {
    let (src, src_db) = seeded("lk-pw-src");
    let file = make_backup(&src_db, &src);
    let (dir, db) = locked_dir("lk-pw", |d| std::fs::write(d.join("db.key"), b"junk").unwrap());
    let before = snapshot_dir(&dir);
    assert!(run_restore(&db, &dir, &file, "wrong password").is_err());
    assert_eq!(snapshot_dir(&dir), before);
}

#[test]
fn restore_while_conversion_was_postponed_encrypts_the_new_db_and_the_kept_old_one() {
    // 전환을 미룬(평문) 상태에서 복원한다: 새 DB는 암호화, 보관된 이전 평문 DB도 곧바로 암호화된다.
    let (src, src_db) = seeded("pp-src");
    let file = make_backup(&src_db, &src);
    let dir = tmp("pp-dst").join("data");
    {
        std::fs::create_dir_all(dir.join("media")).unwrap();
        let mut c = Connection::open(dir.join("tonghap.db")).unwrap();
        crate::db::prepare(&c).unwrap();
        crate::db::testing::seed_year(&mut c, &dir.join("media"), &crate::db::testing::SeedOpts { seed: 3, students: 4, observations: 10, attachments: 0, units: 2, sessions: 6, habits: 1, habit_days: 5, parent_reports: 1, big_unit_sessions: 3 }).unwrap();
    }
    struct NoExport;
    impl crypt::ConvertOps for NoExport {
        fn export(&self, _: &Path, _: &Path, _: &DbKey) -> DbResult<()> {
            Err("주입".to_string())
        }
    }
    let o = crate::db::open_full_with(&dir, &NoExport).unwrap();
    assert!(matches!(o.state, DbState::Plain(_)));
    let db = Mutex::new(o.conn);

    run_restore(&db, &dir, &file, PW).unwrap();
    assert_eq!(counts(&db), counts(&src_db));
    assert_encrypted(&dir);
    let kept = dir_names(&dir, "before-restore-");
    assert_eq!(kept.len(), 1);
    assert_eq!(crypt::file_kind(&kept[0].join("tonghap.db")).unwrap(), FileKind::Encrypted, "보관된 이전 DB도 암호화");
    drop(db);
    let o = crate::db::open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
}

#[test]
fn cleanup_removes_temp_folders_left_by_a_crash() {
    let dir = tmp("cleanup");
    for n in [".backup-tmp", restore::STAGING] {
        std::fs::create_dir_all(dir.join(n)).unwrap();
        std::fs::write(dir.join(n).join("tonghap.db"), b"SQLite format 3\0 plain").unwrap();
    }
    std::fs::create_dir_all(dir.join("media")).unwrap();
    cleanup_leftovers(&dir);
    assert!(!dir.join(".backup-tmp").exists() && !dir.join(restore::STAGING).exists());
    assert!(dir.join("media").exists());
    cleanup_leftovers(&dir); // 없어도 오류 없이 지나간다
}
