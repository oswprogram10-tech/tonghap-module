//! 암호화 백업과 복원. 순수 로직은 `format`(만들기·미리보기)과 `restore`(복원·교체)에 두고,
//! 여기서는 공용 형식과 Tauri 명령만 둔다.

pub mod format;
pub mod restore;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use age::secrecy::SecretString;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::db::{lock, settings, DbExt, DbResult, DbState, DbStatus, MediaDir};

pub const FORMAT_NAME: &str = "tonghap-backup";
pub const FORMAT_VERSION: i64 = 1;
pub const MANIFEST: &str = "manifest.json";
pub const DB_FILE: &str = "tonghap.db";

pub const ERR_PASSWORD: &str = "암호가 틀렸습니다";
pub const ERR_NOT_BACKUP: &str = "이 앱의 백업 파일이 아닙니다";
pub const ERR_CORRUPT: &str = "백업 파일이 손상되었습니다";
pub const ERR_NEWER: &str = "더 새로운 버전 앱에서 만든 백업입니다. 앱을 업데이트해 주세요";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub classes: i64,
    pub students: i64,
    pub observations: i64,
    pub attachments: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub format_version: i64,
    pub app_version: String,
    pub schema_version: i64,
    pub created_at: String,
    pub counts: Counts,
    pub files: Vec<FileEntry>,
    /// DB에는 있으나 디스크에서 찾지 못해 넣지 못한 첨부 경로
    #[serde(default)]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub format_version: i64,
    pub app_version: String,
    pub schema_version: i64,
    pub created_at: String,
    pub counts: Counts,
    pub file_count: usize,
    pub total_bytes: u64,
}

impl BackupSummary {
    pub fn from_manifest(m: &Manifest) -> Self {
        Self {
            format_version: m.format_version,
            app_version: m.app_version.clone(),
            schema_version: m.schema_version,
            created_at: m.created_at.clone(),
            counts: m.counts,
            file_count: m.files.len(),
            total_bytes: m.files.iter().map(|f| f.bytes).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub path: String,
    pub bytes: u64,
    pub counts: Counts,
    pub missing: Vec<String>,
}

/// `stage`: snapshot | hashing | writing | reading | verifying | swapping. 단위는 바이트(snapshot, swapping은 0/1).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgress {
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
}

/// 화면으로 보내는 진행률 이벤트의 최소 간격. 작은 첨부 파일이 수천 개여도 화면이 버벅이지 않게 한다.
pub const PROGRESS_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// 진행률 이벤트를 `min` 간격으로 솎는다. 단계가 바뀔 때와 단계가 끝날 때(`done >= total`)는 항상 통과시킨다.
pub struct Throttle {
    min: std::time::Duration,
    last: Option<(std::time::Instant, &'static str)>,
}

impl Throttle {
    pub fn new(min: std::time::Duration) -> Self {
        Self { min, last: None }
    }

    pub fn pass(&mut self, p: &BackupProgress) -> bool {
        self.pass_at(p, std::time::Instant::now())
    }

    pub fn pass_at(&mut self, p: &BackupProgress, now: std::time::Instant) -> bool {
        let ok = match self.last {
            None => true,
            Some((t, stage)) => stage != p.stage || p.done >= p.total || now.saturating_duration_since(t) >= self.min,
        };
        if ok {
            self.last = Some((now, p.stage));
        }
        ok
    }
}

pub fn progress(stage: &'static str, done: u64, total: u64) -> BackupProgress {
    BackupProgress { stage, done, total }
}

/// 암호는 8자 이상이고 공백만으로 이루어지지 않아야 한다.
pub fn validate_password(pw: &str) -> DbResult<()> {
    if pw.trim().is_empty() || pw.chars().count() < 8 {
        return Err("암호는 8자 이상이어야 합니다(공백만은 안 됩니다)".to_string());
    }
    Ok(())
}

pub fn secret(pw: &str) -> SecretString {
    SecretString::from(pw.to_string())
}

/// 상대 경로가 `media/` 아래 안전한 경로 조각인지(절대 경로·`..`·역슬래시·빈 조각 금지).
pub fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains(':')
        && rel.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LastBackup {
    pub at: String,
    pub path: String,
    pub observations: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub last_backup: Option<LastBackup>,
    pub observations_since: i64,
}

pub fn status(conn: &Connection) -> DbResult<BackupStatus> {
    let last: Option<LastBackup> = serde_json::from_value(settings::get(conn, "lastBackup")?).ok();
    let since: i64 = match &last {
        Some(l) => conn
            .query_row(
                "SELECT count(*) FROM observations WHERE deleted_at IS NULL AND datetime(created_at) > datetime(?1)",
                [&l.at],
                |r| r.get(0),
            )
            .db()?,
        None => conn.query_row("SELECT count(*) FROM observations WHERE deleted_at IS NULL", [], |r| r.get(0)).db()?,
    };
    Ok(BackupStatus { last_backup: last, observations_since: since })
}

/// 백업·복원은 한 번에 하나만 실행한다.
static BUSY: AtomicBool = AtomicBool::new(false);

struct BusyGuard;

impl BusyGuard {
    fn acquire() -> DbResult<Self> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err("다른 백업 또는 복원이 진행 중입니다".to_string());
        }
        Ok(Self)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// 앱을 시작할 때 이전 실행이 도중에 꺼져 남은 임시 폴더(평문 DB 복사본이 들어 있을 수 있다)를 지운다.
/// 복원·백업 중에 부르면 안 된다(시작할 때 한 번만).
pub fn cleanup_leftovers(data_dir: &Path) {
    for name in [".backup-tmp", restore::STAGING] {
        let p = data_dir.join(name);
        if p.exists() {
            let _ = std::fs::remove_dir_all(&p);
            crate::logging::info("backup", "이전 실행에서 남은 임시 폴더를 지웠습니다");
        }
    }
}

/// 기록을 열 수 없는 상태(잠김)에서는 백업을 만들지 않는다(빈 DB를 백업하게 되므로).
fn ensure_not_locked(app: &AppHandle) -> DbResult<()> {
    if let Some(s) = app.try_state::<DbStatus>() {
        if matches!(crate::db::status_of(&s.0), DbState::Locked(_)) {
            return Err("기록을 열 수 없는 상태에서는 백업을 만들 수 없습니다".to_string());
        }
    }
    Ok(())
}

fn data_dir(app: &AppHandle) -> DbResult<PathBuf> {
    app.state::<MediaDir>()
        .0
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "데이터 폴더를 찾을 수 없습니다".to_string())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> DbResult<T> + Send + 'static) -> DbResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("작업이 중단되었습니다: {e}"))?
}

/// 백업·복원 결과를 오류 기록에 남긴다(경로·암호는 남기지 않는다).
fn log_backup_result<T>(what: &str, result: &DbResult<T>) {
    match result {
        Ok(_) => crate::logging::info("backup", &format!("{what} 완료")),
        Err(e) => crate::logging::error("backup", &format!("{what} 실패: {e}")),
    }
}

#[tauri::command]
pub async fn backup_create(app: AppHandle, path: String, password: String, on_progress: Channel<BackupProgress>) -> DbResult<BackupResult> {
    let _busy = BusyGuard::acquire()?;
    ensure_not_locked(&app)?;
    let dir = data_dir(&app)?;
    let mut out = PathBuf::from(path);
    if out.extension().is_none() {
        out.set_extension("tonghap");
    }
    crate::logging::info("backup", "백업 만들기 시작");
    let result = blocking(move || {
        let db = app.state::<Mutex<Connection>>();
        let mut throttle = Throttle::new(PROGRESS_MIN_INTERVAL);
        format::create(&db, &dir, &out, &password, None, &mut |p| {
            if throttle.pass(&p) {
                let _ = on_progress.send(p);
            }
        })
    })
    .await;
    log_backup_result("백업 만들기", &result);
    result
}

#[tauri::command]
pub async fn backup_inspect(path: String, password: String) -> DbResult<BackupSummary> {
    blocking(move || format::inspect(Path::new(&path), &password)).await
}

#[tauri::command]
pub async fn backup_restore(app: AppHandle, path: String, password: String, on_progress: Channel<BackupProgress>) -> DbResult<BackupSummary> {
    let _busy = BusyGuard::acquire()?;
    let dir = data_dir(&app)?;
    crate::logging::info("backup", "백업 복원 시작");
    let handle = app.clone();
    let result = blocking(move || {
        let db = app.state::<Mutex<Connection>>();
        let mut throttle = Throttle::new(PROGRESS_MIN_INTERVAL);
        restore::restore(&db, &dir, Path::new(&path), &password, &mut |p| {
            if throttle.pass(&p) {
                let _ = on_progress.send(p);
            }
        })
    })
    .await;
    if result.is_ok() {
        // 복원된 DB는 항상 암호화되어 열린다(잠김·평문 경고 상태가 풀린다).
        if let Some(s) = handle.try_state::<DbStatus>() {
            *s.0.lock().unwrap_or_else(|e| e.into_inner()) = DbState::Ok;
        }
    }
    log_backup_result("백업 복원", &result);
    result
}

#[tauri::command]
pub fn backup_status(db: tauri::State<Mutex<Connection>>) -> DbResult<BackupStatus> {
    status(&*lock(&db)?)
}

#[cfg(test)]
mod tests;
