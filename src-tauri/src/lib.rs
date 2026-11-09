mod about;
mod backup;
mod db;
mod diag;
mod embed;
mod engine;
mod llm_server;
mod logging;
mod paths;
mod stt;
mod support;
mod voice;
mod webdriver;
#[doc(hidden)]
pub mod testing;

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
    logging::install_panic_hook();
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            let local = paths::local_data_dir(app.handle())?;
            logging::init(local.join("logs"));
            logging::info(
                "app",
                &format!(
                    "시작: 앱 v{} / {}",
                    app.package_info().version,
                    sysinfo::System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.to_string())
                ),
            );
            let data_dir = local.join("data");
            backup::cleanup_leftovers(&data_dir);
            let opened = logging::log_err("db", "데이터베이스 열기", db::open_full(&data_dir))?;
            let (conn, media, db_state) = (opened.conn, opened.media, opened.state);
            logging::info("db", &format!("기록 보호 상태: {}", db_state.kind()));
            logging::info(
                "db",
                &format!("DB 스키마 v{} (최신 v{})", support::read_schema_version(&conn).unwrap_or(-1), db::migrations::latest_version()),
            );
            // 열 수 없는 상태(잠김)에서는 학생 이름을 읽지 않는다. 화면은 안내와 복원만 보여 준다.
            if !matches!(db_state, db::DbState::Locked(_)) {
                logging::refresh_names(&conn);
            }
            app.manage(std::sync::Mutex::new(conn));
            app.manage(media);
            // 사진·음성 암호화(설계 21): 키는 키 파일에서 다시 읽고, 기존 평문 파일은 DB가 정상일 때만 백그라운드에서 바꾼다.
            let media_key = db::media::load_key(&data_dir);
            let convert = matches!(db_state, db::DbState::Ok);
            let media_progress = std::sync::Arc::new(db::media::Progress::default());
            app.manage(db::media::MediaKeyHolder::new(media_key.clone()));
            app.manage(media_progress.clone());
            db::media::start(&data_dir, media_key, convert, media_progress);
            app.manage(db::DbStatus(std::sync::Mutex::new(db_state)));
            stt::start(app.handle())?;
            llm_server::start(app.handle());
            embed::start(app.handle());
            // 메인 창은 상태를 모두 등록한 뒤 만든다(tauri.conf.json의 create:false).
            webdriver::create_main_window(app.handle())?;
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol(db::media_serve::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || responder.respond(db::media_serve::respond(&app, &request)));
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            webdriver::webdriver_overrides_active,
            webdriver::board_open_with_overrides,
            webdriver::e2e_dialog_next,
            webdriver::e2e_debug_hooks,
            about::about_storage,
            db::db_state,
            db::media::media_protection,
            logging::log_frontend_error,
            support::support_preview,
            support::support_create,
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
            db::observations::observation_count,
            db::attachments::attachment_add_file,
            db::attachments::attachment_add_bytes,
            db::attachments::attachment_delete,
            db::attachments::attachment_get,
            db::attachments::attachment_set_transcript,
            stt::stt_status,
            stt::stt_set_model,
            stt::stt_set_auto,
            stt::stt_transcribe,
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
            db::activities::activity_custom_list,
            db::activities::activity_custom_save,
            db::activities::activity_custom_delete,
            db::activities::activity_recommend,
            embed::embed_status,
            embed::question_cluster,
            db::habits::habit_list,
            db::habits::habit_save,
            db::habits::habit_delete,
            db::habits::habit_day,
            db::habits::habit_toggle,
            db::habits::habit_week,
            db::habits::habit_student_summary,
            db::reports::report_draft,
            db::reports::report_save,
            db::reports::report_mark_exported,
            db::reports::report_write_file,
            db::reports::report_default_period,
            db::evaluation::eval_settings_get,
            db::evaluation::eval_settings_set,
            db::evaluation::eval_status,
            db::evaluation::eval_view,
            db::evaluation::eval_generate,
            db::evaluation::llm::eval_llm_status,
            db::evaluation::llm::eval_llm_cancel,
            db::evaluation::llm::eval_generate_llm,
            db::evaluation::eval_save,
            db::evaluation::eval_check,
            db::evaluation::eval_confirm,
            db::evaluation::eval_export,
            db::evaluation::eval_combined_view,
            db::evaluation::eval_combined_status,
            db::evaluation::eval_combine,
            db::evaluation::eval_combined_save,
            db::evaluation::eval_combined_check,
            db::evaluation::eval_combined_confirm,
            db::evaluation::eval_combined_unconfirm,
            db::evaluation::eval_behavior_view,
            db::evaluation::eval_behavior_status,
            db::evaluation::eval_behavior_generate,
            db::evaluation::behavior_llm::eval_behavior_generate_llm,
            db::evaluation::eval_behavior_save,
            db::evaluation::eval_behavior_check,
            db::evaluation::eval_behavior_confirm,
            db::evaluation::eval_behavior_target_set,
            db::settings::settings_get,
            db::settings::settings_set,
            voice::voice_get,
            voice::voice_set,
            voice::voice_clear,
            backup::backup_create,
            backup::backup_inspect,
            backup::backup_restore,
            backup::backup_status,
            db::testing::dev_seed_year,
        ])
        .build(tauri::generate_context!())
        .expect("앱 실행 중 오류")
        .run(|app, event| {
            // 앱이 끝나면 언어 모델 서버(llama-server)도 함께 끈다.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                logging::info("app", "종료");
                if let Some(server) = app.try_state::<std::sync::Arc<llm_server::LlmServer>>() {
                    server.shutdown();
                }
                if let Some(server) = app.try_state::<embed::EmbedServer>() {
                    server.0.shutdown();
                }
            }
        });
}
