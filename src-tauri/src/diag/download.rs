//! 측정용 모델 내려받기(이어받기·SHA-256 확인)와 직접 선택한 파일 가져오기.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

use super::system::describe_reqwest_error;
use super::{model_path, model_spec, DiagResult, ModelKind, ModelSpec, MODELS};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ModelKind,
    pub file_name: &'static str,
    pub size: u64,
    pub license: &'static str,
    pub ready: bool,
    /// 이어받을 수 있는 임시 파일 크기
    pub partial_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "stage")]
pub enum DownloadProgress {
    Downloading { downloaded: u64, total: u64, bytes_per_sec: f64 },
    Verifying,
}

static IN_PROGRESS: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

struct InProgressGuard(&'static str);

impl InProgressGuard {
    fn acquire(id: &'static str) -> DiagResult<Self> {
        let mut set = IN_PROGRESS.lock().unwrap();
        let set = set.get_or_insert_with(HashSet::new);
        if !set.insert(id) {
            return Err("이미 이 모델을 준비하는 중입니다".to_string());
        }
        Ok(Self(id))
    }
}

impl Drop for InProgressGuard {
    fn drop(&mut self) {
        if let Some(set) = IN_PROGRESS.lock().unwrap().as_mut() {
            set.remove(self.0);
        }
    }
}

fn marker_path(path: &Path) -> PathBuf {
    path.with_extension("verified")
}

fn part_path(path: &Path) -> PathBuf {
    path.with_extension("part")
}

/// 크기와 확인 표시 파일이 모두 맞으면 준비된 것으로 본다(매번 해시를 다시 계산하지 않기 위해).
fn is_ready(path: &Path, spec: &ModelSpec) -> bool {
    let size_ok = std::fs::metadata(path).map(|m| m.len() == spec.size).unwrap_or(false);
    let marker_ok = std::fs::read_to_string(marker_path(path))
        .map(|s| s.trim() == spec.sha256)
        .unwrap_or(false);
    size_ok && marker_ok
}

#[tauri::command]
pub fn diag_models(app: AppHandle) -> DiagResult<Vec<ModelStatus>> {
    MODELS
        .iter()
        .map(|spec| {
            let path = model_path(&app, spec)?;
            Ok(ModelStatus {
                id: spec.id,
                label: spec.label,
                kind: spec.kind,
                file_name: spec.file_name,
                size: spec.size,
                license: spec.license,
                ready: is_ready(&path, spec),
                partial_bytes: std::fs::metadata(part_path(&path)).map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn diag_download_model(app: AppHandle, id: String, on_progress: Channel<DownloadProgress>) -> DiagResult<()> {
    let spec = model_spec(&id)?;
    let _guard = InProgressGuard::acquire(spec.id)?;
    let path = model_path(&app, spec)?;
    if is_ready(&path, spec) {
        return Ok(());
    }
    let part = part_path(&path);

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let mut existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    if existing > spec.size {
        let _ = tokio::fs::remove_file(&part).await;
        existing = 0;
    }

    let mut req = client.get(spec.url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let resp = req.send().await.map_err(|e| describe_reqwest_error(&e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("내려받기 실패: HTTP {}", status.as_u16()));
    }
    // 서버가 이어받기를 지원하지 않으면(200) 처음부터 다시 받는다.
    let resume = existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resume {
        existing = 0;
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resume)
        .truncate(!resume)
        .open(&part)
        .await
        .map_err(|e| format!("임시 파일을 만들 수 없음({}): {e}", part.display()))?;

    let mut downloaded = existing;
    let started = Instant::now();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| describe_reqwest_error(&e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("파일 쓰기 실패(디스크 공간 확인): {e}"))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(250) {
            last_emit = Instant::now();
            let secs = started.elapsed().as_secs_f64().max(0.001);
            let _ = on_progress.send(DownloadProgress::Downloading {
                downloaded,
                total: spec.size,
                bytes_per_sec: (downloaded - existing) as f64 / secs,
            });
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);

    if downloaded != spec.size {
        return Err(format!(
            "내려받은 크기가 다름({downloaded} / {} 바이트). 다시 시도하면 이어받습니다",
            spec.size
        ));
    }

    let _ = on_progress.send(DownloadProgress::Verifying);
    finish(part, path, spec).await
}

/// 인터넷이 막힌 PC를 위해 USB 등에서 모델 파일을 직접 가져온다.
#[tauri::command]
pub async fn diag_import_model(app: AppHandle, id: String, source: String) -> DiagResult<()> {
    let spec = model_spec(&id)?;
    let _guard = InProgressGuard::acquire(spec.id)?;
    let path = model_path(&app, spec)?;
    let src = PathBuf::from(source);
    let len = std::fs::metadata(&src).map_err(|e| format!("파일을 열 수 없음: {e}"))?.len();
    if len != spec.size {
        return Err(format!(
            "파일 크기가 다릅니다({len} 바이트). '{}' 파일이 맞는지 확인하세요",
            spec.file_name
        ));
    }
    let part = part_path(&path);
    tokio::fs::copy(&src, &part)
        .await
        .map_err(|e| format!("파일 복사 실패: {e}"))?;
    finish(part, path, spec).await
}

async fn finish(part: PathBuf, path: PathBuf, spec: &'static ModelSpec) -> DiagResult<()> {
    let hash = tauri::async_runtime::spawn_blocking({
        let part = part.clone();
        move || sha256_file(&part)
    })
    .await
    .map_err(|e| e.to_string())??;

    if hash != spec.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err("파일 검증 실패(SHA-256 불일치). 파일이 손상됐거나 다른 파일입니다".to_string());
    }
    std::fs::rename(&part, &path).map_err(|e| format!("파일 이동 실패: {e}"))?;
    std::fs::write(marker_path(&path), spec.sha256).map_err(|e| e.to_string())?;
    Ok(())
}

fn sha256_file(path: &Path) -> DiagResult<String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
