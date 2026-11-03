//! 백업 파일 만들기와 미리보기. tar 묶음을 age(scrypt 암호)로 감싼 스트림이다.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use age::{scrypt, DecryptError, Decryptor, Encryptor, Identity, Recipient};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::*;
use crate::db::{attachments, lock, now_iso, DbExt, DbResult};

const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

pub fn sha256_file(path: &Path, on_bytes: &mut dyn FnMut(u64)) -> DbResult<(u64, String)> {
    let mut f = File::open(path).map_err(|e| format!("파일을 읽을 수 없습니다({}): {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("파일을 읽을 수 없습니다({}): {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        on_bytes(n as u64);
    }
    Ok((total, hex::encode(hasher.finalize())))
}

/// 읽은 바이트를 진행 상황으로 알리는 래퍼(256KB마다).
struct ProgressRead<'a, R> {
    inner: R,
    done: &'a mut u64,
    total: u64,
    last: u64,
    cb: &'a mut dyn FnMut(BackupProgress),
}

impl<R: Read> Read for ProgressRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        *self.done += n as u64;
        if *self.done - self.last >= 256 * 1024 || (n == 0 && *self.done != self.last) {
            self.last = *self.done;
            (self.cb)(progress("writing", *self.done, self.total));
        }
        Ok(n)
    }
}

fn part_path(out: &Path) -> PathBuf {
    let mut s = out.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

struct Item {
    tar_path: String,
    abs: PathBuf,
    bytes: u64,
}

/// `work_factor`는 테스트에서만 낮춘다(실제 앱은 `None`, age 기본값).
pub fn create(
    db: &Mutex<Connection>,
    data_dir: &Path,
    out: &Path,
    password: &str,
    work_factor: Option<u8>,
    cb: &mut dyn FnMut(BackupProgress),
) -> DbResult<BackupResult> {
    validate_password(password)?;
    let tmp = data_dir.join(".backup-tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("임시 폴더를 만들 수 없습니다: {e}"))?;
    let part = part_path(out);
    let result = create_inner(db, data_dir, &tmp, out, &part, password, work_factor, cb);
    let _ = std::fs::remove_dir_all(&tmp);
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn create_inner(
    db: &Mutex<Connection>,
    data_dir: &Path,
    tmp: &Path,
    out: &Path,
    part: &Path,
    password: &str,
    work_factor: Option<u8>,
    cb: &mut dyn FnMut(BackupProgress),
) -> DbResult<BackupResult> {
    // 1) 스냅숏: 잠금은 이 블록 안에서만 쥔다.
    cb(progress("snapshot", 0, 1));
    let snap = tmp.join(DB_FILE);
    let (counts, schema_version, rels) = {
        let conn = lock(db)?;
        let literal = snap.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{literal}'")).db()?;
        let count = |sql: &str| -> DbResult<i64> { conn.query_row(sql, [], |r| r.get(0)).db() };
        let counts = Counts {
            classes: count("SELECT count(*) FROM classes")?,
            students: count("SELECT count(*) FROM students")?,
            observations: count("SELECT count(*) FROM observations WHERE deleted_at IS NULL")?,
            attachments: count("SELECT count(*) FROM attachments")?,
        };
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).db()?;
        let mut stmt = conn.prepare("SELECT DISTINCT rel_path FROM attachments ORDER BY rel_path").db()?;
        let rels: Vec<String> = stmt.query_map([], |r| r.get(0)).db()?.collect::<Result<_, _>>().db()?;
        (counts, version, rels)
    };
    cb(progress("snapshot", 1, 1));

    // 2) 첨부 파일 확인과 해시
    let media = data_dir.join("media");
    let mut items = vec![Item { tar_path: DB_FILE.to_string(), abs: snap.clone(), bytes: 0 }];
    let mut missing = Vec::new();
    let mut seen = HashSet::new();
    for rel in rels {
        if !seen.insert(rel.clone()) {
            continue;
        }
        let abs = attachments::abs_path(&media, &rel);
        if safe_rel(&rel) && abs.is_file() {
            items.push(Item { tar_path: format!("media/{rel}"), abs, bytes: 0 });
        } else {
            missing.push(rel);
        }
    }
    let hash_total: u64 = items.iter().map(|i| std::fs::metadata(&i.abs).map(|m| m.len()).unwrap_or(0)).sum();
    let mut hashed = 0u64;
    let mut files = Vec::new();
    for item in &mut items {
        let (bytes, sha) = sha256_file(&item.abs, &mut |n| {
            hashed += n;
            cb(progress("hashing", hashed, hash_total));
        })?;
        item.bytes = bytes;
        files.push(FileEntry { path: item.tar_path.clone(), bytes, sha256: sha });
    }
    cb(progress("hashing", hash_total, hash_total));

    let manifest = Manifest {
        format: FORMAT_NAME.to_string(),
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version,
        created_at: now_iso(),
        counts,
        files,
        missing: missing.clone(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;

    // 3) tar 작성 → age 암호화 → .part → 이름 변경
    let write_total: u64 = items.iter().map(|i| i.bytes).sum();
    let file = File::create(part).map_err(|e| format!("백업 파일을 만들 수 없습니다({}): {e}", part.display()))?;
    let mut recipient = scrypt::Recipient::new(secret(password));
    if let Some(n) = work_factor {
        recipient.set_work_factor(n);
    }
    let encryptor = Encryptor::with_recipients(iter::once(&recipient as &dyn Recipient)).map_err(|e| e.to_string())?;
    let io = |e: std::io::Error| format!("백업 파일을 쓰지 못했습니다: {e}");
    let stream = encryptor.wrap_output(BufWriter::new(file)).map_err(io)?;
    let mut tar = tar::Builder::new(stream);
    append(&mut tar, MANIFEST, manifest_bytes.len() as u64, &manifest_bytes[..]).map_err(io)?;
    let mut done = 0u64;
    cb(progress("writing", 0, write_total));
    for item in &items {
        let f = File::open(&item.abs).map_err(|e| format!("파일을 읽을 수 없습니다({}): {e}", item.abs.display()))?;
        let reader = ProgressRead { inner: f, done: &mut done, total: write_total, last: 0, cb };
        append(&mut tar, &item.tar_path, item.bytes, reader).map_err(io)?;
    }
    tar.finish().map_err(io)?;
    let stream = tar.into_inner().map_err(io)?;
    let mut writer = stream.finish().map_err(io)?;
    writer.flush().map_err(io)?;
    writer.get_ref().sync_all().map_err(io)?;
    drop(writer);
    std::fs::rename(part, out).map_err(|e| format!("백업 파일 이름을 바꾸지 못했습니다: {e}"))?;
    cb(progress("writing", write_total, write_total));

    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    let path = out.to_string_lossy().into_owned();
    settings::set(
        &*lock(db)?,
        "lastBackup",
        &serde_json::json!({ "at": now_iso(), "path": path, "observations": counts.observations }),
    )?;
    Ok(BackupResult { path, bytes, counts, missing })
}

fn append<W: Write, R: Read>(tar: &mut tar::Builder<W>, path: &str, size: u64, data: R) -> std::io::Result<()> {
    let mut h = tar::Header::new_gnu();
    h.set_size(size);
    h.set_mode(0o644);
    h.set_mtime(0);
    tar.append_data(&mut h, path, data)
}

fn map_decrypt_err(e: DecryptError) -> String {
    match e {
        DecryptError::DecryptionFailed | DecryptError::NoMatchingKeys | DecryptError::KeyDecryptionFailed => ERR_PASSWORD.to_string(),
        _ => ERR_CORRUPT.to_string(),
    }
}

/// 백업 파일을 열어 복호화 스트림을 돌려준다.
pub fn open_stream(path: &Path, password: &str) -> DbResult<impl Read> {
    let file = File::open(path).map_err(|e| format!("파일을 열 수 없습니다: {e}"))?;
    let decryptor = Decryptor::new_buffered(BufReader::new(file)).map_err(|_| ERR_NOT_BACKUP.to_string())?;
    if !decryptor.is_scrypt() {
        return Err(ERR_NOT_BACKUP.to_string());
    }
    let identity = scrypt::Identity::new(secret(password));
    decryptor.decrypt(iter::once(&identity as &dyn Identity)).map_err(map_decrypt_err)
}

/// tar의 첫 항목(`manifest.json`)을 읽고 검사한다.
pub fn read_manifest<R: Read>(entry: &mut tar::Entry<'_, R>) -> DbResult<Manifest> {
    let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
    if name != MANIFEST || !entry.header().entry_type().is_file() {
        return Err(ERR_NOT_BACKUP.to_string());
    }
    if entry.size() > MAX_MANIFEST_BYTES {
        return Err(ERR_CORRUPT.to_string());
    }
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).map_err(|_| ERR_CORRUPT.to_string())?;
    let m: Manifest = serde_json::from_slice(&buf).map_err(|_| ERR_NOT_BACKUP.to_string())?;
    if m.format != FORMAT_NAME {
        return Err(ERR_NOT_BACKUP.to_string());
    }
    if m.format_version > FORMAT_VERSION || m.schema_version > crate::db::migrations::latest_version() {
        return Err(ERR_NEWER.to_string());
    }
    Ok(m)
}

pub fn inspect(path: &Path, password: &str) -> DbResult<BackupSummary> {
    let stream = open_stream(path, password)?;
    let mut archive = tar::Archive::new(stream);
    let mut entries = archive.entries().map_err(|_| ERR_CORRUPT.to_string())?;
    let mut first = entries.next().ok_or_else(|| ERR_NOT_BACKUP.to_string())?.map_err(|_| ERR_CORRUPT.to_string())?;
    let manifest = read_manifest(&mut first)?;
    Ok(BackupSummary::from_manifest(&manifest))
}
