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
pub fn overrides_from(args_env: Option<OsString>, udf_env: Option<OsString>) -> Option<Overrides> {
    let extra = args_env.and_then(|v| v.into_string().ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
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
    overrides_from(std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"), std::env::var_os("WEBVIEW2_USER_DATA_FOLDER"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_set_means_no_overrides() {
        assert_eq!(overrides_from(None, None), None);
        assert_eq!(overrides_from(Some("  ".into()), Some(OsString::new())), None);
    }

    #[test]
    fn args_are_appended_to_wry_defaults() {
        let o = overrides_from(Some("--remote-debugging-port=0".into()), None).unwrap();
        assert_eq!(o.args, format!("{WRY_DEFAULT_ARGS} --remote-debugging-port=0"));
        assert_eq!(o.data_dir, None);
    }

    #[test]
    fn data_folder_alone_keeps_default_args() {
        let o = overrides_from(None, Some("C:\\tmp\\udf".into())).unwrap();
        assert_eq!(o.args, WRY_DEFAULT_ARGS);
        assert_eq!(o.data_dir, Some(PathBuf::from("C:\\tmp\\udf")));
    }
}
