use std::time::Duration;

use serde::Serialize;
use sysinfo::{Disks, System};
use tauri::AppHandle;

use super::{models_dir, vulkan, DiagResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCheck {
    pub target: &'static str,
    pub ok: bool,
    pub detail: String,
    pub elapsed_ms: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    pub app_version: String,
    pub os: String,
    pub arch: &'static str,
    pub cpu_brand: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub models_dir: String,
    pub models_dir_writable: bool,
    pub disk_free_bytes: Option<u64>,
    pub webview_version: Option<String>,
    pub vulkan: vulkan::VulkanInfo,
    pub network: NetworkCheck,
}

#[tauri::command]
pub async fn diag_system(app: AppHandle) -> DiagResult<SystemReport> {
    let dir = models_dir(&app)?;
    let network = check_network().await;

    let report = tauri::async_runtime::spawn_blocking(move || {
        let sys = System::new().ok().map(|mut sys| {
            sys.refresh_cpu_all();
            sys.refresh_memory();
            sys
        });

        // 모델 폴더가 있는 디스크의 여유 공간 (가장 길게 일치하는 마운트 지점)
        let disk_free_bytes = Disks::new_with_refreshed_list().ok().and_then(|disks| {
            disks
                .iter()
                .filter(|d| dir.starts_with(d.mount_point()))
                .max_by_key(|d| d.mount_point().as_os_str().len())
                .map(|d| d.available_space())
        });

        let probe = dir.join(".write-test");
        let models_dir_writable = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);

        SystemReport {
            app_version: app.package_info().version.to_string(),
            os: System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.to_string()),
            arch: std::env::consts::ARCH,
            cpu_brand: sys
                .as_ref()
                .and_then(|s| s.cpus().first().map(|c| c.brand().trim().to_string()))
                .unwrap_or_default(),
            physical_cores: System::physical_core_count().ok(),
            logical_cores: sys.as_ref().map(|s| s.cpus().len()).unwrap_or(0),
            total_memory_bytes: sys.as_ref().map(|s| s.total_memory()).unwrap_or(0),
            available_memory_bytes: sys.as_ref().map(|s| s.available_memory()).unwrap_or(0),
            models_dir: dir.to_string_lossy().into_owned(),
            models_dir_writable,
            disk_free_bytes,
            webview_version: tauri::webview_version().ok(),
            vulkan: vulkan::probe(),
            network,
        }
    })
    .await
    .map_err(|e| format!("시스템 정보 수집 실패: {e}"))?;

    Ok(report)
}

/// 모델을 받을 Hugging Face에 연결되는지 확인한다(학교 방화벽 확인용).
async fn check_network() -> NetworkCheck {
    let target = "https://huggingface.co";
    let started = std::time::Instant::now();
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(c) => c,
        Err(e) => {
            return NetworkCheck { target, ok: false, detail: e.to_string(), elapsed_ms: 0 };
        }
    };
    let (ok, detail) = match client.head(target).send().await {
        Ok(r) => (r.status().is_success() || r.status().is_redirection(), format!("HTTP {}", r.status().as_u16())),
        Err(e) => (false, describe_reqwest_error(&e)),
    };
    NetworkCheck { target, ok, detail, elapsed_ms: started.elapsed().as_millis() }
}

pub fn describe_reqwest_error(e: &reqwest::Error) -> String {
    let kind = if e.is_timeout() {
        "시간 초과"
    } else if e.is_connect() {
        "연결 실패(방화벽·프록시 확인)"
    } else {
        "요청 실패"
    };
    // 원인 체인을 모두 붙여 인증서·프록시 문제를 구분할 수 있게 한다.
    let mut msg = format!("{kind}: {e}");
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        msg.push_str(&format!(" ← {s}"));
        src = s.source();
    }
    msg
}
