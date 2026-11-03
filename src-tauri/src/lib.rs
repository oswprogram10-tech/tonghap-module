mod backup;
mod db;
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
        .setup(|app| {
            use tauri::Manager;
            let data_dir = app.path().app_local_data_dir()?.join("data");
            let (conn, media) = db::open(&data_dir)?;
            app.manage(std::sync::Mutex::new(conn));
            app.manage(media);
            Ok(())
        })
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
            db::classes::class_list,
            db::classes::class_create,
            db::classes::class_update,
            db::classes::class_archive,
            db::students::student_list,
            db::students::student_import,
            db::students::student_update,
            db::observations::observation_create,
            db::observations::observation_update,
            db::observations::observation_delete,
            db::observations::observation_restore,
            db::observations::observation_purge,
            db::observations::observation_list,
            db::attachments::attachment_add_file,
            db::attachments::attachment_add_bytes,
            db::attachments::attachment_delete,
            db::portfolio::portfolio_student,
            db::portfolio::portfolio_class,
            db::units::unit_list,
            db::units::unit_create,
            db::units::unit_update,
            db::units::unit_archive,
            db::units::unit_unarchive,
            db::units::unit_delete,
            db::units::unit_get,
            db::units::stage_rename,
            db::units::session_create,
            db::units::session_update,
            db::units::session_move,
            db::units::session_delete,
            db::units::session_complete_and_follow,
            db::units::sessions_for_record,
            db::plays::play_custom_list,
            db::plays::play_custom_save,
            db::plays::play_custom_delete,
            db::plays::play_log_add,
            db::habits::habit_list,
            db::habits::habit_save,
            db::habits::habit_delete,
            db::habits::habit_day,
            db::habits::habit_toggle,
            db::habits::habit_week,
            db::habits::habit_student_summary,
            db::settings::settings_get,
            db::settings::settings_set,
            backup::backup_create,
            backup::backup_inspect,
            backup::backup_restore,
            backup::backup_status,
        ])
        .run(tauri::generate_context!())
        .expect("앱 실행 중 오류");
}
