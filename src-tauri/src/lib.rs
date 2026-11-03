mod diag;

use serde::Serialize;

#[derive(Serialize)]
struct AppInfo {
    version: String,
    os: &'static str,
    arch: &'static str,
}

/// 화면 하단에 표시할 앱 버전·실행 환경.
#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            diag::system::diag_system,
            diag::bench::diag_engines,
            diag::download::diag_models,
            diag::download::diag_download_model,
            diag::download::diag_import_model,
            diag::bench::diag_bench_llm,
            diag::bench::diag_bench_whisper,
            diag::diag_save_report,
        ])
        .run(tauri::generate_context!())
        .expect("앱 실행 중 오류");
}
