use std::cell::Cell;
use std::collections::BTreeMap;

use super::*;
use crate::db::key::{self, DbKey, KEY_FILE};
use crate::db::testing::{seed_year, SeedOpts};
use crate::db::{new_id, open, open_full, open_full_with, DbState};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tonghap-crypt-{}-{}", name, new_id()));
    std::fs::create_dir_all(d.join("media")).unwrap();
    d
}

fn small() -> SeedOpts {
    SeedOpts { seed: 7, students: 6, observations: 40, attachments: 5, units: 3, sessions: 12, habits: 2, habit_days: 10, parent_reports: 3, big_unit_sessions: 4 }
}

/// 평문 DB(`dir/tonghap.db`)를 만들고 표별 행 수를 돌려준다. 연결은 닫는다.
fn seed_plain(dir: &Path) -> Vec<(String, i64)> {
    let mut conn = Connection::open(dir.join(DB_NAME)).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    crate::db::prepare(&conn).unwrap();
    seed_year(&mut conn, &dir.join("media"), &small()).unwrap();
    let c = table_counts(&conn).unwrap();
    drop(conn);
    c
}

fn counts_enc(path: &Path, key: &DbKey) -> Vec<(String, i64)> {
    table_counts(&open_keyed(path, key).unwrap()).unwrap()
}

fn counts_plain(path: &Path) -> Vec<(String, i64)> {
    table_counts(&Connection::open(path).unwrap()).unwrap()
}

/// 폴더 안의 모든 파일(하위 폴더 포함) 내용.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn names_in_plain(dir: &Path) -> Vec<String> {
    let c = Connection::open(dir.join(DB_NAME)).unwrap();
    let mut stmt = c.prepare("SELECT name FROM students").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

fn assert_no_leftovers(dir: &Path) {
    for n in [ENC_TMP, PLAIN_OLD, "tonghap.db.plain-old-wal", "tonghap.db.plain-old-shm"] {
        assert!(!dir.join(n).exists(), "{n} 가 남아 있음");
    }
}

// ---------- 새 DB ----------

#[test]
fn new_database_is_encrypted_and_hides_names() {
    let dir = tmp("new");
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    let key = match key::read_key_file(&dir) {
        key::KeyFile::Key(k) => k,
        other => panic!("{other:?}"),
    };
    let name = "김하늘잘한다";
    o.conn.execute("INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 't')", []).unwrap();
    o.conn.execute("INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s', 'c', 1, ?1, 't')", [name]).unwrap();
    // 쓰기가 끝난 뒤 연결이 열려 있을 때와 닫은 뒤, 두 번 모두 평문이 없다.
    let check = |when: &str| {
        for (file, bytes) in snapshot(&dir) {
            // -shm은 윈도우에서 열린 동안 바이트 범위가 잠겨 읽을 수 없다(내용은 메모리 색인뿐이다).
            if file.starts_with("media") || file == KEY_FILE || file.ends_with("-shm") {
                continue;
            }
            assert!(!contains(&bytes, name.as_bytes()), "{file} 에 이름이 그대로 보임({when})");
            if file == DB_NAME {
                assert_ne!(&bytes[..16], SQLITE_MAGIC, "첫 16바이트가 평문 SQLite 머리({when})");
            }
        }
    };
    check("열린 채");
    drop(o.conn);
    check("닫은 뒤");
    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    let c = open_keyed(&dir.join(DB_NAME), &key).unwrap();
    let got: String = c.query_row("SELECT name FROM students", [], |r| r.get(0)).unwrap();
    assert_eq!(got, name);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn existing_key_without_database_is_reused() {
    let dir = tmp("keyonly");
    let k = DbKey::generate().unwrap();
    key::write_key_file(&dir, &k).unwrap();
    let before = std::fs::read(dir.join(KEY_FILE)).unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    drop(o.conn);
    assert_eq!(std::fs::read(dir.join(KEY_FILE)).unwrap(), before);
    assert!(open_keyed(&dir.join(DB_NAME), &k).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reopening_an_encrypted_database_keeps_the_data() {
    let dir = tmp("reopen");
    let o = open_full(&dir).unwrap();
    o.conn.execute("INSERT INTO settings (key, value) VALUES ('k', '1')", []).unwrap();
    drop(o.conn);
    let (conn, _) = open(&dir).unwrap();
    let v: String = conn.query_row("SELECT value FROM settings WHERE key = 'k'", [], |r| r.get(0)).unwrap();
    assert_eq!(v, "1");
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(fk, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 전환 ----------

#[test]
fn plain_database_is_converted_with_identical_row_counts() {
    let dir = tmp("convert");
    let before = seed_plain(&dir);
    let names = names_in_plain(&dir);
    assert!(before.iter().any(|(_, n)| *n > 0));
    assert!(contains(&std::fs::read(dir.join(DB_NAME)).unwrap(), names[0].as_bytes()), "원본에는 이름이 보인다(검사가 의미 있음)");

    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    drop(o.conn);

    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    assert_no_leftovers(&dir);
    let key = match key::read_key_file(&dir) {
        key::KeyFile::Key(k) => k,
        other => panic!("{other:?}"),
    };
    assert_eq!(counts_enc(&dir.join(DB_NAME), &key), before);
    // 평문 흔적이 어떤 파일에도 없다.
    for (file, bytes) in snapshot(&dir) {
        if file.starts_with("media") || file == KEY_FILE {
            continue;
        }
        for n in &names {
            assert!(!contains(&bytes, n.as_bytes()), "{file} 에 이름 {n} 이 보임");
        }
    }
    let uv: i64 = open_keyed(&dir.join(DB_NAME), &key).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(uv, crate::db::migrations::latest_version());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 평문이 그대로이고 열리며 내용이 같다.
fn assert_plain_intact(dir: &Path, before: &[(String, i64)], bytes: &[u8]) {
    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Plain);
    assert_eq!(std::fs::read(dir.join(DB_NAME)).unwrap(), bytes, "평문 파일이 바뀌었음");
    assert_eq!(counts_plain(&dir.join(DB_NAME)), before);
    assert_no_leftovers(dir);
}

struct Faults {
    fail_export: bool,
    drop_row_after_export: bool,
    fail_rename_at: Option<usize>,
    tamper_on_final_rename: bool,
    free: Option<u64>,
    calls: Cell<usize>,
}

impl Faults {
    fn none() -> Self {
        Self { fail_export: false, drop_row_after_export: false, fail_rename_at: None, tamper_on_final_rename: false, free: None, calls: Cell::new(0) }
    }
}

fn drop_one_observation(path: &Path, key: &DbKey) {
    let c = open_keyed(path, key).unwrap();
    c.execute("DELETE FROM observations WHERE id IN (SELECT id FROM observations LIMIT 1)", []).unwrap();
}

impl ConvertOps for Faults {
    fn export(&self, plain: &Path, dest: &Path, key: &DbKey) -> DbResult<()> {
        if self.fail_export {
            return Err("주입한 실패".to_string());
        }
        export_to_encrypted(plain, dest, key)?;
        if self.drop_row_after_export {
            drop_one_observation(dest, key);
        }
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> DbResult<()> {
        let n = self.calls.get() + 1;
        self.calls.set(n);
        if self.fail_rename_at == Some(n) {
            return Err("주입한 이름 바꾸기 실패".to_string());
        }
        if self.tamper_on_final_rename && from.file_name().is_some_and(|f| f == ENC_TMP) {
            drop_one_observation(from, &KEY.with(|k| k.clone()));
        }
        std::fs::rename(from, to).map_err(|e| e.to_string())
    }

    fn free_space(&self, _dir: &Path) -> Option<u64> {
        self.free
    }
}

thread_local! {
    static KEY: DbKey = DbKey::generate().unwrap();
}

fn run(dir: &Path, ops: &dyn ConvertOps) -> Converted {
    convert_plain_db(&dir.join(DB_NAME), &KEY.with(|k| k.clone()), ops)
}

#[test]
fn export_failure_keeps_plain_database() {
    let dir = tmp("f-export");
    let before = seed_plain(&dir);
    let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    let r = run(&dir, &Faults { fail_export: true, ..Faults::none() });
    assert!(matches!(r, Converted::Skipped(_)), "{r:?}");
    assert_plain_intact(&dir, &before, &bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn row_count_mismatch_keeps_plain_database() {
    let dir = tmp("f-count");
    let before = seed_plain(&dir);
    let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    let r = run(&dir, &Faults { drop_row_after_export: true, ..Faults::none() });
    match r {
        Converted::Skipped(why) => assert!(why.contains("원본과 같지 않아"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_plain_intact(&dir, &before, &bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_failures_roll_back_to_the_plain_database() {
    for at in [1usize, 2] {
        let dir = tmp("f-rename");
        let before = seed_plain(&dir);
        let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
        let r = run(&dir, &Faults { fail_rename_at: Some(at), ..Faults::none() });
        assert!(matches!(r, Converted::Skipped(_)), "at={at} {r:?}");
        assert_plain_intact(&dir, &before, &bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn failed_final_verification_restores_the_plain_database() {
    let dir = tmp("f-verify");
    let before = seed_plain(&dir);
    let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    // 이름을 바꾸는 순간 새 파일의 내용이 달라졌다고 가정한다(바꾼 뒤 확인에서 걸려야 한다).
    let r = run(&dir, &Faults { tamper_on_final_rename: true, ..Faults::none() });
    match r {
        Converted::Skipped(why) => assert!(why.contains("되돌렸습니다"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_plain_intact(&dir, &before, &bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn low_disk_space_postpones_the_conversion() {
    let dir = tmp("f-disk");
    let before = seed_plain(&dir);
    let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    let r = run(&dir, &Faults { free: Some(1000), ..Faults::none() });
    match r {
        Converted::Skipped(why) => assert!(why.contains("디스크 여유"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_plain_intact(&dir, &before, &bytes);
    // 충분하면 전환된다.
    let r = run(&dir, &Faults { free: Some(u64::MAX), ..Faults::none() });
    assert_eq!(r, Converted::Done);
    assert_eq!(counts_enc(&dir.join(DB_NAME), &KEY.with(|k| k.clone())), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn damaged_plain_database_is_not_converted() {
    let dir = tmp("f-corrupt");
    let before = seed_plain(&dir);
    // 가운데 쪽 페이지들을 망가뜨린다.
    let mut bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    let len = bytes.len();
    assert!(len > 4096 * 40, "{len}");
    for b in &mut bytes[4096 * 3..4096 * 30] {
        *b = 0xAB;
    }
    std::fs::write(dir.join(DB_NAME), &bytes).unwrap();
    let r = run(&dir, &Faults::none());
    assert!(matches!(r, Converted::Skipped(_)), "{r:?}");
    assert_eq!(std::fs::read(dir.join(DB_NAME)).unwrap(), bytes, "손상된 파일을 건드리면 안 됨");
    assert_no_leftovers(&dir);
    let _ = before;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn leftover_plain_old_blocks_conversion_and_is_never_overwritten() {
    let dir = tmp("f-oldexists");
    let before = seed_plain(&dir);
    std::fs::write(dir.join(PLAIN_OLD), b"someone elses file").unwrap();
    let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    let r = run(&dir, &Faults::none());
    assert!(matches!(r, Converted::Skipped(_)));
    assert_eq!(std::fs::read(dir.join(PLAIN_OLD)).unwrap(), b"someone elses file");
    assert_eq!(std::fs::read(dir.join(DB_NAME)).unwrap(), bytes);
    assert_eq!(counts_plain(&dir.join(DB_NAME)), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn skipped_conversion_opens_as_plain_state_and_retries_next_time() {
    let dir = tmp("f-open");
    let before = seed_plain(&dir);
    let o = open_full_with(&dir, &Faults { fail_export: true, ..Faults::none() }).unwrap();
    assert!(matches!(&o.state, DbState::Plain(r) if !r.is_empty()), "{:?}", o.state);
    assert!(!o.state.encrypted());
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    drop(o.conn);
    assert_no_leftovers(&dir);
    // 다음 열기에서 이어서 전환된다.
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 중간에 꺼진 경우 ----------

#[test]
fn leftover_enc_tmp_is_removed_and_conversion_restarts() {
    let dir = tmp("r-tmp");
    let before = seed_plain(&dir);
    std::fs::write(dir.join(ENC_TMP), b"half written").unwrap();
    std::fs::write(dir.join("tonghap.db.enc-tmp-wal"), b"x").unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    drop(o.conn);
    assert_no_leftovers(&dir);
    assert!(!dir.join("tonghap.db.enc-tmp-wal").exists());
    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_plain_old_left_is_put_back_and_converted() {
    // 평문을 plain-old로 옮긴 직후, 새 파일을 놓기 전에 꺼진 상태.
    let dir = tmp("r-old");
    let before = seed_plain(&dir);
    std::fs::rename(dir.join(DB_NAME), dir.join(PLAIN_OLD)).unwrap();
    std::fs::write(dir.join(ENC_TMP), b"whatever").unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before, "예전 평문 기록이 되살아나 전환되어야 함");
    drop(o.conn);
    assert_no_leftovers(&dir);
    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_old_next_to_a_good_encrypted_database_is_deleted() {
    // 새 파일을 놓은 뒤 평문 보관본을 지우기 전에 꺼진 상태.
    let dir = tmp("r-done");
    let before = seed_plain(&dir);
    let k = DbKey::generate().unwrap();
    key::write_key_file(&dir, &k).unwrap();
    export_to_encrypted(&dir.join(DB_NAME), &dir.join(ENC_TMP), &k).unwrap();
    std::fs::rename(dir.join(DB_NAME), dir.join(PLAIN_OLD)).unwrap();
    std::fs::rename(dir.join(ENC_TMP), dir.join(DB_NAME)).unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    drop(o.conn);
    assert_no_leftovers(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_old_is_kept_when_the_encrypted_database_differs() {
    let dir = tmp("r-differs");
    seed_plain(&dir);
    let k = DbKey::generate().unwrap();
    key::write_key_file(&dir, &k).unwrap();
    export_to_encrypted(&dir.join(DB_NAME), &dir.join(ENC_TMP), &k).unwrap();
    drop_one_observation(&dir.join(ENC_TMP), &k);
    std::fs::rename(dir.join(DB_NAME), dir.join(PLAIN_OLD)).unwrap();
    std::fs::rename(dir.join(ENC_TMP), dir.join(DB_NAME)).unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    drop(o.conn);
    assert!(dir.join(PLAIN_OLD).exists(), "다르면 평문 보관본을 지우면 안 됨");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 열 수 없음 ----------

fn encrypted_dir(name: &str) -> (PathBuf, Vec<(String, i64)>) {
    let dir = tmp(name);
    let before = seed_plain(&dir);
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    drop(o.conn);
    (dir, before)
}

fn assert_locked_and_untouched(dir: &Path) {
    let files = snapshot(dir);
    let o = open_full(dir).unwrap();
    assert!(matches!(o.state, DbState::Locked(_)), "{:?}", o.state);
    assert!(o.state.encrypted());
    // 자리 표시 DB는 메모리이고 비어 있다.
    let n: i64 = o.conn.query_row("SELECT count(*) FROM students", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    drop(o.conn);
    assert_eq!(snapshot(dir), files, "잠긴 상태에서 파일이 바뀌었거나 새로 생겼음");
    assert!(open(dir).is_err());
    assert_eq!(snapshot(dir), files);
}

#[test]
fn missing_key_with_encrypted_database_is_locked_and_nothing_changes() {
    let (dir, _) = encrypted_dir("l-missing");
    std::fs::remove_file(dir.join(KEY_FILE)).unwrap();
    assert_locked_and_untouched(&dir);
    assert!(!dir.join(KEY_FILE).exists(), "새 키를 만들면 안 됨");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wrong_key_is_locked_and_nothing_changes() {
    let (dir, _) = encrypted_dir("l-wrong");
    key::write_key_file(&dir, &DbKey::generate().unwrap()).unwrap();
    assert_locked_and_untouched(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreadable_key_file_is_locked_and_nothing_changes() {
    let (dir, _) = encrypted_dir("l-junk");
    std::fs::write(dir.join(KEY_FILE), b"junk").unwrap();
    assert_locked_and_untouched(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn correct_key_unlocks_again_after_being_locked() {
    let (dir, before) = encrypted_dir("l-again");
    let good = std::fs::read(dir.join(KEY_FILE)).unwrap();
    std::fs::write(dir.join(KEY_FILE), b"junk").unwrap();
    assert!(matches!(open_full(&dir).unwrap().state, DbState::Locked(_)));
    std::fs::write(dir.join(KEY_FILE), good).unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreadable_key_with_plain_database_is_set_aside_and_replaced() {
    let dir = tmp("k-plain");
    let before = seed_plain(&dir);
    std::fs::write(dir.join(KEY_FILE), b"junk").unwrap();
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(table_counts(&o.conn).unwrap(), before);
    drop(o.conn);
    let aside = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("db.key.unreadable-")).count();
    assert_eq!(aside, 1, "풀 수 없던 키 파일은 지우지 않고 남긴다");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 예전 복원 보관 폴더 ----------

#[test]
fn plain_database_in_old_before_restore_folder_is_encrypted_too() {
    let dir = tmp("old-folder");
    let old = dir.join("before-restore-20270101-000000");
    std::fs::create_dir_all(&old).unwrap();
    let before = seed_plain(&old);
    let names = names_in_plain(&old);
    let o = open_full(&dir).unwrap();
    assert_eq!(o.state, DbState::Ok);
    drop(o.conn);
    assert_eq!(file_kind(&old.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    let key = match key::read_key_file(&dir) {
        key::KeyFile::Key(k) => k,
        other => panic!("{other:?}"),
    };
    assert_eq!(counts_enc(&old.join(DB_NAME), &key), before);
    assert!(!contains(&std::fs::read(old.join(DB_NAME)).unwrap(), names[0].as_bytes()));
    assert_no_leftovers(&old);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn old_folder_conversion_failure_does_not_stop_startup() {
    let dir = tmp("old-folder-fail");
    let old = dir.join("before-restore-20270101-000000");
    std::fs::create_dir_all(&old).unwrap();
    let before = seed_plain(&old);
    let bytes = std::fs::read(old.join(DB_NAME)).unwrap();
    let o = open_full_with(&dir, &Faults { fail_export: true, ..Faults::none() }).unwrap();
    assert_eq!(o.state, DbState::Ok);
    drop(o.conn);
    assert_plain_intact(&old, &before, &bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 내보내기 ----------

#[test]
fn export_to_plain_makes_a_plain_sqlite_copy_and_detaches() {
    let (dir, before) = encrypted_dir("x-plain");
    let (conn, _) = open(&dir).unwrap();
    let out = dir.join("copy.db");
    export_to_plain(&conn, &out).unwrap();
    assert_eq!(file_kind(&out).unwrap(), FileKind::Plain);
    assert_eq!(counts_plain(&out), before);
    let uv: i64 = Connection::open(&out).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(uv, crate::db::migrations::latest_version());
    // 붙인 DB가 남지 않아 원래 연결로 계속 쓸 수 있다.
    export_to_plain(&conn, &dir.join("copy2.db")).unwrap();
    conn.execute("INSERT INTO settings (key, value) VALUES ('after', '1')", []).unwrap();
    // 실패해도 붙은 채로 남지 않는다.
    assert!(export_to_plain(&conn, &dir.join("no-such-dir").join("x.db")).is_err());
    export_to_plain(&conn, &dir.join("copy3.db")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn encrypt_file_in_place_replaces_a_plain_file_and_keeps_it_on_failure() {
    let dir = tmp("x-inplace");
    let before = seed_plain(&dir);
    let k = DbKey::generate().unwrap();
    encrypt_file_in_place(&dir.join(DB_NAME), &k).unwrap();
    assert_eq!(file_kind(&dir.join(DB_NAME)).unwrap(), FileKind::Encrypted);
    assert_eq!(counts_enc(&dir.join(DB_NAME), &k), before);
    assert!(!with_suffix(&dir.join(DB_NAME), ".enc").exists());
    // 이미 암호화된 파일은 평문으로 읽을 수 없어 실패하고, 파일은 그대로다.
    let bytes = std::fs::read(dir.join(DB_NAME)).unwrap();
    assert!(encrypt_file_in_place(&dir.join(DB_NAME), &k).is_err());
    assert_eq!(std::fs::read(dir.join(DB_NAME)).unwrap(), bytes);
    assert!(!with_suffix(&dir.join(DB_NAME), ".enc").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_kind_classifies_files() {
    let dir = tmp("kind");
    assert_eq!(file_kind(&dir.join("none")).unwrap(), FileKind::Missing);
    std::fs::write(dir.join("empty"), b"").unwrap();
    assert_eq!(file_kind(&dir.join("empty")).unwrap(), FileKind::Missing);
    std::fs::write(dir.join("short"), b"abc").unwrap();
    assert_eq!(file_kind(&dir.join("short")).unwrap(), FileKind::Encrypted);
    std::fs::write(dir.join("plain"), b"SQLite format 3\0rest").unwrap();
    assert_eq!(file_kind(&dir.join("plain")).unwrap(), FileKind::Plain);
    let _ = std::fs::remove_dir_all(&dir);
}
