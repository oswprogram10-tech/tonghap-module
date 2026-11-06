//! 백업 복원: 임시 폴더에 풀고, 검증하고, 열려 있는 DB 연결을 닫은 채 파일을 교체한다.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use super::format::{open_stream, read_manifest, sha256_file};
use super::*;
use crate::db::key::{self, DbKey, KeyFile};
use crate::db::media::{self, MediaKey};
use crate::db::{self, crypt, lock, migrations, DbExt, DbResult};

pub const STAGING: &str = ".restore-staging";
pub const KEEP_BEFORE_RESTORE: usize = 3;
const BEFORE_PREFIX: &str = "before-restore-";
/// 현재 기록을 열 수 없을 때(잠김) 그 파일을 옮겨 두는 폴더 이름의 머리. 자동으로 지우지 않는다.
const UNREADABLE_PREFIX: &str = "unreadable-";

fn bad_content(why: &str) -> String {
    format!("백업 파일의 내용이 올바르지 않습니다({why}). 손상되었거나 변조된 파일일 수 있습니다")
}

/// tar 항목 이름이 `manifest.json`, `tonghap.db`, `media/…` 중 하나인 안전한 경로인지 확인한다.
fn allowed_name(name: &str) -> bool {
    if !safe_rel(name) {
        return false;
    }
    name == MANIFEST || name == DB_FILE || name.strip_prefix("media/").is_some_and(|rest| !rest.is_empty())
}

fn io_corrupt<T>(r: std::io::Result<T>) -> DbResult<T> {
    r.map_err(|_| ERR_CORRUPT.to_string())
}

fn staging_path(staging: &Path, name: &str) -> PathBuf {
    name.split('/').fold(staging.to_path_buf(), |p, part| p.join(part))
}

/// 풀기 → 검증까지. 성공하면 검증된 매니페스트를 돌려준다. `staging`은 호출자가 지운다.
pub fn extract_and_verify(path: &Path, password: &str, staging: &Path, cb: &mut dyn FnMut(BackupProgress)) -> DbResult<Manifest> {
    let _ = std::fs::remove_dir_all(staging);
    std::fs::create_dir_all(staging).map_err(|e| format!("임시 폴더를 만들 수 없습니다: {e}"))?;

    cb(progress("reading", 0, 0));
    let stream = open_stream(path, password)?;
    let mut archive = tar::Archive::new(stream);
    let manifest;
    {
        let mut entries = io_corrupt(archive.entries())?;
        let mut first = entries.next().ok_or_else(|| ERR_NOT_BACKUP.to_string())?.map_err(|_| ERR_CORRUPT.to_string())?;
        manifest = read_manifest(&mut first)?;
        drop(first);

        let mut expected: HashMap<&str, &FileEntry> = HashMap::new();
        for f in &manifest.files {
            if !allowed_name(&f.path) || f.path == MANIFEST || expected.insert(&f.path, f).is_some() {
                return Err(bad_content("목록의 파일 이름"));
            }
        }
        if !expected.contains_key(DB_FILE) {
            return Err(bad_content("데이터베이스 없음"));
        }
        let total: u64 = manifest.files.iter().map(|f| f.bytes).sum();
        let mut done = 0u64;
        cb(progress("reading", 0, total));

        for entry in entries {
            let mut entry = entry.map_err(|_| ERR_CORRUPT.to_string())?;
            let name = String::from_utf8(entry.path_bytes().into_owned()).map_err(|_| bad_content("파일 이름"))?;
            if !entry.header().entry_type().is_file() || !allowed_name(&name) || name == MANIFEST {
                return Err(bad_content("허용되지 않는 항목"));
            }
            let spec = expected.remove(name.as_str()).ok_or_else(|| bad_content("목록에 없는 파일"))?;
            if entry.size() != spec.bytes {
                return Err(bad_content("파일 크기"));
            }
            let dest = staging_path(staging, &name);
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("임시 폴더를 만들 수 없습니다: {e}"))?;
            }
            let mut out = BufWriter::new(File::create(&dest).map_err(|e| format!("파일을 쓸 수 없습니다: {e}"))?);
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = io_corrupt(entry.read(&mut buf))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).map_err(|e| format!("파일을 쓸 수 없습니다: {e}"))?;
                done += n as u64;
                cb(progress("reading", done, total));
            }
            out.flush().map_err(|e| format!("파일을 쓸 수 없습니다: {e}"))?;
        }
        if !expected.is_empty() {
            return Err(bad_content("빠진 파일"));
        }
    }
    // 끝까지 읽어야 age가 잘림·변조를 검출한다.
    io_corrupt(std::io::copy(&mut archive.into_inner(), &mut std::io::sink()))?;

    let total: u64 = manifest.files.iter().map(|f| f.bytes).sum();
    let mut done = 0u64;
    cb(progress("verifying", 0, total));
    for f in &manifest.files {
        let (bytes, sha) = sha256_file(&staging_path(staging, &f.path), &mut |n| {
            done += n;
            cb(progress("verifying", done, total));
        })?;
        if bytes != f.bytes || !sha.eq_ignore_ascii_case(&f.sha256) {
            return Err(bad_content("파일 해시 불일치"));
        }
    }
    check_db(&staging.join(DB_FILE))?;
    Ok(manifest)
}

/// 풀린 db가 온전한지 확인하고 필요하면 마이그레이션한다.
fn check_db(path: &Path) -> DbResult<()> {
    let conn = Connection::open(path).map_err(|_| bad_content("데이터베이스"))?;
    let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(|_| bad_content("데이터베이스"))?;
    if ok != "ok" {
        return Err(bad_content("데이터베이스 검사 실패"));
    }
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).db()?;
    if version > migrations::latest_version() {
        return Err(ERR_NEWER.to_string());
    }
    migrations::apply(&conn)?;
    conn.close().map_err(|(_, e)| format!("데이터베이스 오류: {e}"))
}

fn mv(from: &Path, to: &Path) -> DbResult<()> {
    std::fs::rename(from, to).map_err(|e| format!("파일을 옮기지 못했습니다({}): {e}", from.display()))
}

fn unique_dir(data_dir: &Path, prefix: &str, stamp: &str) -> PathBuf {
    let base = data_dir.join(format!("{prefix}{stamp}"));
    let mut dir = base.clone();
    let mut n = 2;
    while dir.exists() {
        dir = PathBuf::from(format!("{}-{n}", base.display()));
        n += 1;
    }
    dir
}

/// 지금 `data/tonghap.db`를 이 앱이 열 수 있는지(없거나 평문이거나, 키로 풀림).
fn current_db_readable(data_dir: &Path) -> bool {
    match crypt::file_kind(&data_dir.join(DB_FILE)) {
        Ok(crypt::FileKind::Missing | crypt::FileKind::Plain) => true,
        Ok(crypt::FileKind::Encrypted) => match key::read_key_file(data_dir) {
            KeyFile::Key(k) => crypt::open_keyed(&data_dir.join(DB_FILE), &k).is_ok(),
            _ => false,
        },
        Err(_) => false,
    }
}

/// 복원할 DB를 암호화할 키. 지금 키 파일이 풀리면 그것을 쓰고, 아니면(잠김) 새 키를 만들어
/// `staging/db.key`에 둔다. 교체할 때 옛 키 파일은 보관 폴더로 옮겨지고 새 키가 자리에 놓인다.
fn restore_key(data_dir: &Path, staging: &Path) -> DbResult<DbKey> {
    match key::read_key_file(data_dir) {
        KeyFile::Key(k) => Ok(k),
        _ => {
            let k = DbKey::generate()?;
            key::write_key_file_at(&staging.join(key::KEY_FILE), &k)?;
            Ok(k)
        }
    }
}

/// staging의 평문 사진·음성을 암호문으로 바꾼다. 파일마다 암호화 → 복호화 해시 확인 → 덮어쓰기.
/// 너무 큰 파일(64MB 초과)은 평문 그대로 둔다(읽을 때는 평문도 열린다). 실패하면 오류.
pub fn encrypt_staged_media(dir: &Path, key: &MediaKey, ops: &dyn media::FileOps, cb: &mut dyn FnMut(BackupProgress)) -> DbResult<()> {
    let files = media::list_files(dir);
    let total = files.len() as u64;
    cb(progress("encrypting", 0, total));
    for (i, (rel, abs)) in files.iter().enumerate() {
        match media::convert_file(key, abs, rel, ops) {
            media::Outcome::Converted | media::Outcome::Already => {}
            media::Outcome::Skipped(why) => crate::logging::info("backup", &format!("복원: 사진·음성 한 개를 암호화하지 않았습니다({why})")),
            media::Outcome::Failed(why) => return Err(format!("사진·음성을 암호화하지 못해 복원하지 않았습니다: {why}")),
        }
        cb(progress("encrypting", i as u64 + 1, total));
    }
    Ok(())
}

/// 현재 DB·첨부를 `before-restore-<stamp>`로 옮기고 staging의 것으로 바꾼다. 실패하면 원래대로 되돌린다.
/// 지금 기록을 열 수 없던 경우(잠김)에는 `unreadable-<stamp>`로 옮기고, staging에 새 키(`db.key`)가 있으면 옛 키 파일도 함께 옮긴다.
pub fn swap_data(db: &Mutex<Connection>, data_dir: &Path, staging: &Path, stamp: &str) -> DbResult<PathBuf> {
    swap_data_with(db, data_dir, staging, stamp, &mv)
}

/// `swap_data`의 본체. 테스트에서 이동 실패를 흉내 낼 수 있도록 이동 함수를 받는다.
pub fn swap_data_with(
    db: &Mutex<Connection>,
    data_dir: &Path,
    staging: &Path,
    stamp: &str,
    mv: &dyn Fn(&Path, &Path) -> DbResult<()>,
) -> DbResult<PathBuf> {
    let mut guard = lock(db)?;
    let _ = guard.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
    // 파일 핸들을 닫기 위해 메모리 DB로 바꾼다(윈도우는 열린 파일을 옮길 수 없다).
    let old = std::mem::replace(&mut *guard, Connection::open_in_memory().db()?);
    let _ = old.close();

    let new_key = staging.join(key::KEY_FILE);
    let swap_key = new_key.exists();
    let prefix = if current_db_readable(data_dir) { BEFORE_PREFIX } else { UNREADABLE_PREFIX };
    let before = unique_dir(data_dir, prefix, stamp);
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new(); // (원래 위치, 보관 위치)
    let mut placed: Vec<PathBuf> = Vec::new(); // staging에서 data로 옮겨 놓은 것
    let attempt = (|| -> DbResult<Connection> {
        std::fs::create_dir_all(&before).map_err(|e| format!("보관 폴더를 만들 수 없습니다: {e}"))?;
        let mut names = vec!["tonghap.db", "tonghap.db-wal", "tonghap.db-shm", "media"];
        if swap_key {
            names.push(key::KEY_FILE);
        }
        for name in names {
            let from = data_dir.join(name);
            if from.exists() {
                let to = before.join(name);
                mv(&from, &to)?;
                moved.push((from, to));
            }
        }
        mv(&staging.join(DB_FILE), &data_dir.join("tonghap.db"))?;
        placed.push(data_dir.join("tonghap.db"));
        if swap_key {
            mv(&new_key, &data_dir.join(key::KEY_FILE))?;
            placed.push(data_dir.join(key::KEY_FILE));
        }
        let new_media = staging.join("media");
        if new_media.exists() {
            mv(&new_media, &data_dir.join("media"))?;
            placed.push(data_dir.join("media"));
        }
        db::open(data_dir).map(|(c, _)| c)
    })();

    match attempt {
        Ok(conn) => {
            *guard = conn;
            Ok(before)
        }
        Err(err) => {
            // 새로 놓은 것만 지운다. 아직 옮기지 못한 원래 파일은 그 자리에 그대로 둔다.
            // 새 db를 놓았다면 원래 db·-wal·-shm은 이미 모두 옮겨진 뒤이므로,
            // data에 있는 -wal/-shm은 db::open이 새 db에 만든 것이다.
            if !placed.is_empty() {
                for name in ["tonghap.db-wal", "tonghap.db-shm"] {
                    let _ = std::fs::remove_file(data_dir.join(name));
                }
            }
            for p in placed.iter().rev() {
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(p);
                } else {
                    let _ = std::fs::remove_file(p);
                }
            }
            for (from, to) in moved.iter().rev() {
                let _ = std::fs::rename(to, from);
            }
            let _ = std::fs::remove_dir(&before);
            if let Ok((conn, _)) = db::open(data_dir) {
                *guard = conn;
            }
            Err(format!("복원을 마치지 못해 원래 데이터로 되돌렸습니다: {err}"))
        }
    }
}

/// `before-restore-*` 폴더를 최신 `keep`개만 남긴다.
pub fn prune_before_restore(data_dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(data_dir) else { return };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(BEFORE_PREFIX) && e.path().is_dir())
        .map(|e| e.path())
        .collect();
    dirs.sort();
    let excess = dirs.len().saturating_sub(keep);
    for d in dirs.into_iter().take(excess) {
        let _ = std::fs::remove_dir_all(d);
    }
}

pub fn restore(db: &Mutex<Connection>, data_dir: &Path, path: &Path, password: &str, cb: &mut dyn FnMut(BackupProgress)) -> DbResult<BackupSummary> {
    let staging = data_dir.join(STAGING);
    let result = (|| {
        let manifest = extract_and_verify(path, password, &staging, cb)?;
        // 백업 속 DB는 평문이다. 지금 키로 암호화한 파일로 바꾼 뒤 교체한다.
        let key = restore_key(data_dir, &staging)?;
        crypt::encrypt_file_in_place(&staging.join(DB_FILE), &key)?;
        // 사진·음성도 같은 키에서 나온 미디어 키로 암호화한다(백업 안은 평문). 하나라도 실패하면 교체하지 않는다.
        encrypt_staged_media(&staging.join("media"), &MediaKey::from_db_key(&key), &media::RealFileOps, cb)?;
        cb(progress("swapping", 0, 1));
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        swap_data(db, data_dir, &staging, &stamp)?;
        // 백업에 대기·진행 중인 자막이 있어도 이 앱에는 대기열이 없으므로 되돌린다.
        if let Ok(conn) = db.lock() {
            let _ = crate::stt::reset_pending(&conn);
        }
        prune_before_restore(data_dir, KEEP_BEFORE_RESTORE);
        cb(progress("swapping", 1, 1));
        Ok(BackupSummary::from_manifest(&manifest))
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}
