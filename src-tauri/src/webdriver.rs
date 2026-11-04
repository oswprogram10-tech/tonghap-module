//! 화면 E2E(WebDriver)용 WebView2 설정. 디버그 빌드에서만 켜지고 릴리스 빌드에서는 항상 꺼져 있다.
//!
//! 이유: 관리자 권한(High IL)으로 도는 프로세스(GitHub 윈도우 러너)에서는 WebView2가
//! `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, `WEBVIEW2_USER_DATA_FOLDER` 환경 변수를 무시하고
//! 코드로 준 값만 따른다(Microsoft 문서 「Develop secure WebView2 apps」). msedgedriver는 이 두 환경 변수로
//! 원격 디버그 포트와 사용자 데이터 폴더를 알려 주므로, 값을 읽어 창을 만들 때 코드로 다시 넘긴다.

use std::ffi::OsString;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// wry가 기본으로 주는 인자. 인자를 직접 지정하면 기본값이 사라지므로 앞에 붙여 둔다.
const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

#[derive(Debug, PartialEq)]
pub struct Overrides {
    pub args: String,
    pub data_dir: Option<PathBuf>,
}

/// 환경 변수 값에서 덮어쓸 설정을 계산한다(환경을 직접 읽지 않아 테스트하기 쉽다).
pub fn overrides_from(args_env: Option<OsString>, udf_env: Option<OsString>, e2e_args_env: Option<OsString>) -> Option<Overrides> {
    let clean = |v: Option<OsString>| v.and_then(|v| v.into_string().ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    // TONGHAP_E2E_BROWSER_ARGS: 테스트가 더하는 인자(가짜 마이크 등). msedgedriver가 위 환경 변수를 직접 채우므로 따로 받는다.
    let extra = match (clean(args_env), clean(e2e_args_env)) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    };
    let data_dir = udf_env.filter(|v| !v.is_empty()).map(PathBuf::from);
    if extra.is_none() && data_dir.is_none() {
        return None;
    }
    let args = match extra {
        Some(e) => format!("{WRY_DEFAULT_ARGS} {e}"),
        None => WRY_DEFAULT_ARGS.to_string(),
    };
    Some(Overrides { args, data_dir })
}

/// 디버그 빌드에서만 환경 변수를 읽는다.
pub fn overrides() -> Option<Overrides> {
    if !cfg!(debug_assertions) {
        return None;
    }
    overrides_from(
        std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"),
        std::env::var_os("WEBVIEW2_USER_DATA_FOLDER"),
        std::env::var_os("TONGHAP_E2E_BROWSER_ARGS"),
    )
}

fn apply<'a, R: tauri::Runtime, M: Manager<R>>(mut b: WebviewWindowBuilder<'a, R, M>, o: &Overrides) -> WebviewWindowBuilder<'a, R, M> {
    b = b.additional_browser_args(&o.args);
    if let Some(d) = &o.data_dir {
        b = b.data_directory(d.clone());
    }
    b
}

/// 설정 파일의 첫 창(main)을 만든다(`create: false`). 평소와 같고, E2E 설정이 있을 때만 인자를 더한다.
pub fn create_main_window(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = app.config().app.windows.first().cloned().ok_or("tauri.conf.json에 창 설정이 없습니다")?;
    let mut b = WebviewWindowBuilder::from_config(app, &cfg)?;
    if let Some(o) = overrides() {
        b = apply(b, &o);
    }
    b.build()?;
    Ok(())
}

/// 화면이 칠판 창을 열기 전에 묻는다: true이면 `board_open_with_overrides`로 열어야 한다.
#[tauri::command]
pub fn webdriver_overrides_active() -> bool {
    overrides().is_some()
}

/// 칠판 창을 메인 창과 같은 WebView2 설정으로 연다(다른 인자·데이터 폴더의 웹뷰는 같은 폴더를 함께 쓸 수 없다).
#[tauri::command]
pub async fn board_open_with_overrides(app: AppHandle) -> Result<(), String> {
    let o = overrides().ok_or("E2E 설정이 꺼져 있습니다")?;
    let b = WebviewWindowBuilder::new(&app, "board", WebviewUrl::App("index.html#/board".into()))
        .title("바슬즐 칠판")
        .min_inner_size(900.0, 600.0)
        .inner_size(1280.0, 800.0)
        .maximized(true);
    apply(b, &o).build().map(|_| ()).map_err(|e| format!("칠판 창을 만들 수 없습니다: {e}"))
}

// ---------- 대화상자 대체 (디버그 빌드 + TONGHAP_E2E_DIALOG_DIR) ----------

#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq)]
struct QueuedDialog {
    kind: String,
    paths: Vec<String>,
}

/// `queue.json` 내용에서 맨 앞 항목을 꺼낸다. 돌려주는 값은 (꺼낸 경로들, 남은 큐 JSON).
/// 큐가 비었으면 `None`, 종류가 다르면 오류(항목은 그대로 둔다).
fn pop_queued(queue_json: &str, kind: &str) -> Result<Option<(Vec<String>, String)>, String> {
    let mut queue: Vec<QueuedDialog> = serde_json::from_str(queue_json).map_err(|e| format!("대화상자 큐(queue.json)를 읽을 수 없습니다: {e}"))?;
    let Some(first) = queue.first() else { return Ok(None) };
    if first.kind != kind {
        return Err(format!("대화상자 종류가 맞지 않습니다(앱: {kind}, 큐: {})", first.kind));
    }
    let item = queue.remove(0);
    let rest = serde_json::to_string(&queue).map_err(|e| e.to_string())?;
    Ok(Some((item.paths, rest)))
}

/// 큐 폴더에서 다음 대화상자 결과를 꺼낸다. 릴리스 빌드(`enabled == false`)나 폴더 없음·파일 없음이면 `None`.
fn next_dialog(enabled: bool, dir: Option<PathBuf>, kind: &str) -> Result<Option<Vec<String>>, String> {
    let Some(dir) = dir.filter(|_| enabled) else { return Ok(None) };
    let file = dir.join("queue.json");
    let Ok(text) = std::fs::read_to_string(&file) else { return Ok(None) };
    match pop_queued(&text, kind)? {
        None => Ok(None),
        Some((paths, rest)) => {
            std::fs::write(&file, rest).map_err(|e| format!("대화상자 큐를 저장할 수 없습니다: {e}"))?;
            Ok(Some(paths))
        }
    }
}

static DIALOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 화면의 대화상자 호출 래퍼가 먼저 부른다. E2E 큐에 값이 있으면 그 경로를, 아니면 `null`(진짜 대화상자를 연다).
#[tauri::command]
pub fn e2e_dialog_next(kind: String) -> Result<Option<Vec<String>>, String> {
    let _guard = DIALOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    next_dialog(cfg!(debug_assertions), std::env::var_os("TONGHAP_E2E_DIALOG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from), &kind)
}

/// 자막 테스트용 모델 파일(디버그 빌드 + TONGHAP_E2E_STT_MODEL). 릴리스에서는 항상 `None`.
pub fn stt_model_override() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("TONGHAP_E2E_STT_MODEL").filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_set_means_no_overrides() {
        assert_eq!(overrides_from(None, None, None), None);
        assert_eq!(overrides_from(Some("  ".into()), Some(OsString::new()), None), None);
    }

    #[test]
    fn args_are_appended_to_wry_defaults() {
        let o = overrides_from(Some("--remote-debugging-port=0".into()), None, None).unwrap();
        assert_eq!(o.args, format!("{WRY_DEFAULT_ARGS} --remote-debugging-port=0"));
        assert_eq!(o.data_dir, None);
    }

    #[test]
    fn data_folder_alone_keeps_default_args() {
        let o = overrides_from(None, Some("C:\\tmp\\udf".into()), None).unwrap();
        assert_eq!(o.args, WRY_DEFAULT_ARGS);
        assert_eq!(o.data_dir, Some(PathBuf::from("C:\\tmp\\udf")));
    }

    fn queue(items: &[(&str, &[&str])]) -> String {
        serde_json::to_string(&items.iter().map(|(k, p)| QueuedDialog { kind: k.to_string(), paths: p.iter().map(|s| s.to_string()).collect() }).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn queue_pops_in_order_and_empties() {
        let q = queue(&[("open", &["a.png"]), ("save", &["b.tonghap"])]);
        let (paths, rest) = pop_queued(&q, "open").unwrap().unwrap();
        assert_eq!(paths, vec!["a.png"]);
        let (paths, rest) = pop_queued(&rest, "save").unwrap().unwrap();
        assert_eq!(paths, vec!["b.tonghap"]);
        assert_eq!(pop_queued(&rest, "save").unwrap(), None);
    }

    #[test]
    fn queue_kind_mismatch_is_an_error_and_keeps_the_item() {
        let q = queue(&[("save", &["x"])]);
        let err = pop_queued(&q, "open").unwrap_err();
        assert!(err.contains("종류가 맞지 않습니다"), "{err}");
        assert!(pop_queued(&q, "save").unwrap().is_some());
    }

    #[test]
    fn bad_queue_file_is_an_error() {
        assert!(pop_queued("not json", "open").is_err());
    }

    #[test]
    fn next_dialog_is_none_when_disabled_or_unset_or_missing_file() {
        let dir = std::env::temp_dir().join(format!("tonghap-dlg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("queue.json"), queue(&[("open", &["a"])])).unwrap();
        assert_eq!(next_dialog(false, Some(dir.clone()), "open").unwrap(), None); // 릴리스
        assert_eq!(next_dialog(true, None, "open").unwrap(), None); // 환경 변수 없음
        let empty = std::env::temp_dir().join(format!("tonghap-dlg-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(next_dialog(true, Some(empty.clone()), "open").unwrap(), None); // 파일 없음
        // 릴리스에서는 큐가 그대로 남는다.
        assert_eq!(next_dialog(true, Some(dir.clone()), "open").unwrap(), Some(vec!["a".to_string()]));
        assert_eq!(next_dialog(true, Some(dir.clone()), "open").unwrap(), None); // 비면 null
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn e2e_extra_args_are_appended() {
        let o = overrides_from(Some("--remote-debugging-port=0".into()), None, Some("--use-fake-ui-for-media-stream".into())).unwrap();
        assert_eq!(o.args, format!("{WRY_DEFAULT_ARGS} --remote-debugging-port=0 --use-fake-ui-for-media-stream"));
        let only = overrides_from(None, None, Some("--x".into())).unwrap();
        assert_eq!(only.args, format!("{WRY_DEFAULT_ARGS} --x"));
    }
}
