//! 사진·음성 파일 암호화. 설계: docs/design/21-media-encryption.md.
//!
//! 파일 형식: `b"TGM1"`(4) + nonce(24, 난수) + XChaCha20-Poly1305 암호문+태그. AAD는 첨부의 `rel_path`.
//! 파일 이름·확장자는 그대로 두고 내용만 암호문이다. 읽을 때 앞 4바이트로 판별하므로 평문(예전 파일)도 읽힌다.
//!
//! 기존 평문 파일의 전환 원칙: 암호화해 임시 파일에 쓰고 → 다시 읽어 복호화한 내용의 해시가 원본과 같음을
//! 확인한 **다음에만** 원본 자리를 덮는다. 어느 단계가 실패해도 원본은 그대로다.
//! 키·경로는 로그에 남기지 않는다.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};

use super::key::{self, DbKey, KeyFile, KEY_LEN};
use super::DbResult;

pub const MAGIC: &[u8; 4] = b"TGM1";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// 암호화로 늘어나는 바이트 수(머리 + nonce + 태그)
pub const OVERHEAD: u64 = (4 + NONCE_LEN + TAG_LEN) as u64;
/// 암호화하는 파일 하나의 상한(평문 기준).
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const ENC_TMP_SUFFIX: &str = ".enc-tmp";

#[derive(Clone)]
pub struct MediaKey([u8; KEY_LEN]);

impl std::fmt::Debug for MediaKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MediaKey(가림)")
    }
}

impl MediaKey {
    pub fn from_db_key(k: &DbKey) -> Self {
        Self(k.derive_media_key())
    }

    #[cfg(test)]
    pub fn for_test(n: u8) -> Self {
        Self::from_db_key(&DbKey::from_bytes([n; KEY_LEN]))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.0).into())
    }
}

#[cfg(test)]
pub fn test_key() -> MediaKey {
    MediaKey::for_test(1)
}

/// 키 파일을 읽어 미디어 키를 만든다(풀 수 없으면 `None`).
pub fn load_key(data_dir: &Path) -> Option<MediaKey> {
    match key::read_key_file(data_dir) {
        KeyFile::Key(k) => Some(MediaKey::from_db_key(&k)),
        _ => None,
    }
}

/// 관리 상태로 두는 현재 미디어 키. 복원 뒤 키가 바뀔 수 있어(잠긴 상태에서 복원) 갈아 끼울 수 있다.
pub struct MediaKeyHolder(pub RwLock<Option<MediaKey>>);

impl MediaKeyHolder {
    pub fn new(k: Option<MediaKey>) -> Self {
        Self(RwLock::new(k))
    }

    pub fn get(&self) -> Option<MediaKey> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, k: Option<MediaKey>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = k;
    }

    pub fn require(&self) -> DbResult<MediaKey> {
        self.get().ok_or_else(|| "사진·음성을 보호하는 키를 쓸 수 없어 저장하지 못했습니다".to_string())
    }
}

pub fn is_encrypted_head(head: &[u8]) -> bool {
    head.len() >= MAGIC.len() && &head[..MAGIC.len()] == MAGIC
}

/// 평문 → 파일 내용(머리 + nonce + 암호문).
pub fn encrypt(key: &MediaKey, rel: &str, plain: &[u8]) -> DbResult<Vec<u8>> {
    if plain.len() as u64 > MAX_FILE_BYTES {
        return Err("파일이 너무 큽니다(64MB 이하만 가능)".to_string());
    }
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| "난수를 얻지 못해 파일을 암호화할 수 없습니다".to_string())?;
    let ct = key
        .cipher()
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: rel.as_bytes() })
        .map_err(|_| "파일을 암호화하지 못했습니다".to_string())?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// `TGM1` 파일 내용 → 평문. 키·`rel`이 다르거나 내용이 바뀌었으면 오류.
pub fn decrypt(key: &MediaKey, rel: &str, data: &[u8]) -> DbResult<Vec<u8>> {
    if !is_encrypted_head(data) || data.len() < MAGIC.len() + NONCE_LEN + TAG_LEN {
        return Err("암호화된 파일 형식이 아닙니다".to_string());
    }
    let (nonce, ct) = data[MAGIC.len()..].split_at(NONCE_LEN);
    key.cipher()
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: rel.as_bytes() })
        .map_err(|_| "파일을 복호화하지 못했습니다(손상되었거나 다른 자리의 파일입니다)".to_string())
}

/// 파일 내용을 평문으로. `TGM1`이면 복호화(키 필요), 아니면 그대로(전환 전 파일).
pub fn open_bytes(key: Option<&MediaKey>, rel: &str, data: Vec<u8>) -> DbResult<Vec<u8>> {
    if !is_encrypted_head(&data) {
        return Ok(data);
    }
    let key = key.ok_or("사진·음성을 풀 키를 쓸 수 없습니다")?;
    decrypt(key, rel, &data)
}

fn read_err(e: std::io::Error) -> String {
    format!("파일을 읽을 수 없습니다: {e}")
}

/// 파일을 읽어 평문으로 돌려준다(통째로 메모리에).
pub fn read_plain(key: Option<&MediaKey>, abs: &Path, rel: &str) -> DbResult<Vec<u8>> {
    open_bytes(key, rel, std::fs::read(abs).map_err(read_err)?)
}

/// 평문 크기(진행률 계산용 추정). 암호문이면 머리·태그를 뺀 크기.
pub fn plain_size(abs: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(abs) else { return 0 };
    if head_is_encrypted(abs) {
        meta.len().saturating_sub(OVERHEAD)
    } else {
        meta.len()
    }
}

fn head_is_encrypted(abs: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(abs) else { return false };
    let mut head = [0u8; 4];
    f.read_exact(&mut head).is_ok() && is_encrypted_head(&head)
}

/// 새 파일을 암호화해 쓴다: 임시 파일에 쓰고 `rename`. 실패하면 임시 파일을 지운다.
pub fn write_encrypted(key: &MediaKey, abs: &Path, rel: &str, plain: &[u8]) -> DbResult<()> {
    let bytes = encrypt(key, rel, plain)?;
    write_atomic(abs, &bytes)
}

fn tmp_path(abs: &Path) -> PathBuf {
    let mut s = abs.as_os_str().to_owned();
    s.push(ENC_TMP_SUFFIX);
    PathBuf::from(s)
}

fn write_atomic(abs: &Path, bytes: &[u8]) -> DbResult<()> {
    use std::io::Write;
    let tmp = tmp_path(abs);
    let r = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, abs)
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("파일을 저장하지 못했습니다: {e}"));
    }
    Ok(())
}

// ---------- 백업용 읽기 ----------

/// 평문으로 읽을 수 있는 원천. 평문 파일은 스트리밍, 암호문은 풀어서 메모리에 둔다.
pub enum PlainSource {
    File(std::fs::File),
    Memory(std::io::Cursor<Vec<u8>>),
}

impl Read for PlainSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            PlainSource::File(f) => f.read(buf),
            PlainSource::Memory(c) => c.read(buf),
        }
    }
}

pub fn open_plain(key: Option<&MediaKey>, abs: &Path, rel: &str) -> DbResult<PlainSource> {
    use std::io::Seek;
    let mut f = std::fs::File::open(abs).map_err(read_err)?;
    let mut head = [0u8; 4];
    let n = read_up_to(&mut f, &mut head).map_err(read_err)?;
    if !is_encrypted_head(&head[..n]) {
        f.rewind().map_err(read_err)?;
        return Ok(PlainSource::File(f));
    }
    drop(f);
    Ok(PlainSource::Memory(std::io::Cursor::new(read_plain(key, abs, rel)?)))
}

fn read_up_to(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

// ---------- 평문 파일 전환 ----------

/// 전환에서 바깥 세계와 닿는 동작. 테스트가 실패를 끼워 넣을 수 있도록 나누어 두었다.
pub trait FileOps {
    fn write(&self, path: &Path, bytes: &[u8]) -> DbResult<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(path).map_err(|e| format!("임시 파일을 만들 수 없습니다: {e}"))?;
        f.write_all(bytes).map_err(|e| format!("임시 파일을 쓰지 못했습니다: {e}"))?;
        f.sync_all().map_err(|e| format!("임시 파일을 쓰지 못했습니다: {e}"))
    }
    fn read(&self, path: &Path) -> DbResult<Vec<u8>> {
        std::fs::read(path).map_err(read_err)
    }
    /// 임시 파일로 원본 자리를 덮는다(대상이 있으면 바꿔 준다).
    fn replace(&self, from: &Path, to: &Path) -> DbResult<()> {
        std::fs::rename(from, to).map_err(|e| format!("파일 이름을 바꾸지 못했습니다: {e}"))
    }
}

pub struct RealFileOps;
impl FileOps for RealFileOps {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 평문이었던 파일을 암호문으로 바꿨다.
    Converted,
    /// 이미 암호문이다.
    Already,
    /// 일부러 건드리지 않았다(너무 큼). 평문 그대로 읽힌다.
    Skipped(String),
    /// 실패했다. 원본은 그대로이고 임시 파일은 지웠다.
    Failed(String),
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn stamp(abs: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let m = std::fs::metadata(abs).ok()?;
    Some((m.len(), m.modified().ok()))
}

/// 평문 파일 하나를 암호문으로 바꾼다. `rel`은 AAD로 쓰는 상대 경로(`2027/03/<id>.jpg`).
pub fn convert_file(key: &MediaKey, abs: &Path, rel: &str, ops: &dyn FileOps) -> Outcome {
    let Some(before) = stamp(abs) else { return Outcome::Failed("파일을 읽을 수 없습니다".to_string()) };
    if before.0 > MAX_FILE_BYTES {
        return Outcome::Skipped("파일이 너무 커서 암호화하지 않았습니다".to_string());
    }
    let plain = match ops.read(abs) {
        Ok(p) => p,
        Err(e) => return Outcome::Failed(e),
    };
    if is_encrypted_head(&plain) {
        return Outcome::Already;
    }
    let want = sha(&plain);
    let tmp = tmp_path(abs);
    let r = (|| -> DbResult<()> {
        let enc = encrypt(key, rel, &plain)?;
        ops.write(&tmp, &enc)?;
        // 임시 파일을 다시 읽어 풀었을 때 원본과 같은지 확인한다.
        let back = decrypt(key, rel, &ops.read(&tmp)?)?;
        if sha(&back) != want {
            return Err("암호화한 내용이 원본과 달라 바꾸지 않았습니다".to_string());
        }
        // 그 사이 원본이 바뀌었거나 지워졌다면(첨부 삭제 등) 덮지 않는다.
        if stamp(abs) != Some(before) {
            return Err("작업 중 파일이 바뀌어 바꾸지 않았습니다".to_string());
        }
        ops.replace(&tmp, abs)
    })();
    match r {
        Ok(()) => Outcome::Converted,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Outcome::Failed(e)
        }
    }
}

/// 폴더 안의 파일을 (상대 경로, 절대 경로)로 모은다. `*.enc-tmp`는 뺀다.
pub fn list_files(dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(base: &Path, cur: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(cur) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                walk(base, &p, out);
            } else if t.is_file() {
                if p.to_string_lossy().ends_with(ENC_TMP_SUFFIX) {
                    continue;
                }
                let Ok(rel) = p.strip_prefix(base) else { continue };
                let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
                if let Some(parts) = parts {
                    out.push((parts.join("/"), p.clone()));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// 폴더 안에 남은 `*.enc-tmp`(전환 도중 꺼진 흔적)를 지운다. 지운 개수.
pub fn cleanup_tmp(dir: &Path) -> usize {
    fn walk(cur: &Path, n: &mut usize) {
        let Ok(rd) = std::fs::read_dir(cur) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                walk(&p, n);
            } else if t.is_file() && p.to_string_lossy().ends_with(ENC_TMP_SUFFIX) && std::fs::remove_file(&p).is_ok() {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(dir, &mut n);
    n
}

/// (암호화된 파일 수, 평문 파일 수)
pub fn count_state(dir: &Path) -> (u64, u64) {
    let (mut enc, mut plain) = (0, 0);
    for (_, abs) in list_files(dir) {
        if head_is_encrypted(&abs) {
            enc += 1;
        } else {
            plain += 1;
        }
    }
    (enc, plain)
}

// ---------- 진행 상황과 백그라운드 전환 ----------

pub const STATE_WORKING: u8 = 1;
pub const STATE_DONE: u8 = 2;

/// 화면에 알리는 전환 진행 상황(관리 상태).
#[derive(Default)]
pub struct Progress {
    state: AtomicU8,
    total: AtomicU64,
    done: AtomicU64,
    failed: AtomicU64,
    stop: AtomicBool,
}

impl Progress {
    /// 백그라운드 전환을 멈추게 한다(복원이 미디어 폴더를 옮기기 전에). 새 키로 복원한 파일은 이미 암호문이다.
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> (&'static str, u64, u64, u64) {
        let state = match self.state.load(Ordering::SeqCst) {
            STATE_WORKING => "working",
            STATE_DONE => "done",
            _ => "waiting",
        };
        (state, self.done.load(Ordering::SeqCst), self.total.load(Ordering::SeqCst), self.failed.load(Ordering::SeqCst))
    }
}

/// 폴더 안의 평문 파일을 하나씩 전환한다. 실패한 파일은 건너뛰고 계속한다. (전환한 수, 실패한 수)
pub fn convert_dir(dir: &Path, key: &MediaKey, ops: &dyn FileOps, progress: Option<&Progress>) -> (u64, u64) {
    let files: Vec<(String, PathBuf)> = list_files(dir).into_iter().filter(|(_, abs)| !head_is_encrypted(abs)).collect();
    if let Some(p) = progress {
        p.total.store(files.len() as u64, Ordering::SeqCst);
        p.done.store(0, Ordering::SeqCst);
        p.failed.store(0, Ordering::SeqCst);
    }
    let (mut converted, mut failed) = (0, 0);
    for (rel, abs) in files {
        if progress.is_some_and(|p| p.stop.load(Ordering::SeqCst)) {
            break;
        }
        match convert_file(key, &abs, &rel, ops) {
            Outcome::Converted => converted += 1,
            Outcome::Already => {}
            Outcome::Skipped(_) => {
                if let Some(p) = progress {
                    p.failed.fetch_add(1, Ordering::SeqCst);
                }
            }
            Outcome::Failed(why) => {
                failed += 1;
                crate::logging::error("media", &format!("사진·음성 암호화 실패(원본은 그대로): {why}"));
                if let Some(p) = progress {
                    p.failed.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        if let Some(p) = progress {
            p.done.fetch_add(1, Ordering::SeqCst);
        }
    }
    (converted, failed)
}

/// 시작 때 한 번: 남은 `*.enc-tmp`를 정리하고(동기), 백그라운드 스레드가 전환한다.
/// `convert`가 거짓이면(DB가 `Ok`가 아님) 정리만 한다.
pub fn start(data_dir: &Path, key: Option<MediaKey>, convert: bool, progress: std::sync::Arc<Progress>) {
    let media = data_dir.join("media");
    let mut dirs = vec![media.clone()];
    dirs.extend(before_restore_media(data_dir));
    let cleaned: usize = dirs.iter().map(|d| cleanup_tmp(d)).sum();
    if cleaned > 0 {
        crate::logging::info("media", &format!("이전 실행에서 남은 임시 파일 {cleaned}개를 지웠습니다"));
    }
    let (true, Some(key)) = (convert, key) else { return };
    // 화면이 처음 물을 때 「시작 전」으로 보이지 않도록 스레드를 만들기 전에 표시한다.
    progress.state.store(STATE_WORKING, Ordering::SeqCst);
    let done = progress.clone();
    let spawned = std::thread::Builder::new().name("media-encrypt".into()).spawn(move || {
        crate::logging::info("media", "사진·음성 암호화 확인 시작");
        let (n, failed) = convert_dir(&media, &key, &RealFileOps, Some(&progress));
        crate::logging::info("media", &format!("사진·음성 암호화: {n}개 바꿈, 실패 {failed}개"));
        progress.state.store(STATE_DONE, Ordering::SeqCst);
        // 예전 복원 보관 폴더(`before-restore-*`)도 같은 방식으로. 실패해도 계속한다.
        for d in dirs.iter().skip(1) {
            let (n, failed) = convert_dir(d, &key, &RealFileOps, None);
            crate::logging::info("media", &format!("복원 보관 폴더 사진·음성: {n}개 바꿈, 실패 {failed}개"));
        }
    });
    if spawned.is_err() {
        crate::logging::error("media", "사진·음성 암호화 작업을 시작하지 못했습니다");
        done.state.store(STATE_DONE, Ordering::SeqCst);
    }
}

fn before_restore_media(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(data_dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("before-restore-"))
        .map(|e| e.path().join("media"))
        .filter(|p| p.is_dir())
        .collect();
    v.sort();
    v
}

/// 정보 화면의 「기록 보호」 한 줄에 쓰는 사진·음성 상태.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaProtection {
    /// `waiting`(시작 전 또는 전환하지 않음) | `working` | `done`
    pub state: &'static str,
    pub done: u64,
    pub total: u64,
    pub failed: u64,
    pub encrypted: u64,
    pub plain: u64,
}

#[tauri::command]
pub async fn media_protection(media: tauri::State<'_, super::MediaDir>, progress: tauri::State<'_, std::sync::Arc<Progress>>) -> DbResult<MediaProtection> {
    let dir = media.0.clone();
    let (state, done, total, failed) = progress.snapshot();
    let (encrypted, plain) = tauri::async_runtime::spawn_blocking(move || count_state(&dir))
        .await
        .map_err(|e| format!("작업이 중단되었습니다: {e}"))?;
    Ok(MediaProtection { state, done, total, failed, encrypted, plain })
}

#[cfg(test)]
mod tests;
