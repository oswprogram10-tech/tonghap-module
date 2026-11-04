//! 앱 데이터 폴더 위치. 화면 E2E 테스트가 임시 폴더를 쓰도록 환경 변수로 바꿀 수 있다.

use std::ffi::OsString;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// 테스트용 데이터 폴더를 지정하는 환경 변수. 없으면 기본 위치(`app_local_data_dir`)를 쓴다.
pub const DATA_DIR_ENV: &str = "TONGHAP_DATA_DIR";

/// 환경 변수 값이 비어 있지 않으면 그 경로, 아니면 기본 폴더를 돌려준다(환경을 직접 읽지 않아 테스트하기 쉽다).
pub fn resolve_local_data_dir<E>(env_value: Option<OsString>, default: impl FnOnce() -> Result<PathBuf, E>) -> Result<PathBuf, E> {
    match env_value {
        Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => default(),
    }
}

/// 데이터(`data`)·모델(`models`)·로그(`logs`) 폴더의 부모 폴더.
/// 환경 변수 바꾸기는 디버그 빌드(화면 E2E 테스트)에서만 쓴다. 선생님이 쓰는 릴리스 빌드는 항상 기본 위치를 쓴다.
pub fn local_data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    let env_value = if cfg!(debug_assertions) { std::env::var_os(DATA_DIR_ENV) } else { None };
    resolve_local_data_dir(env_value, || app.path().app_local_data_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_value_wins_over_default() {
        let got = resolve_local_data_dir::<String>(Some("/tmp/e2e-data".into()), || Ok(PathBuf::from("/normal"))).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/e2e-data"));
    }

    #[test]
    fn unset_or_empty_uses_default() {
        let got = resolve_local_data_dir::<String>(None, || Ok(PathBuf::from("/normal"))).unwrap();
        assert_eq!(got, PathBuf::from("/normal"));
        let got = resolve_local_data_dir::<String>(Some(OsString::new()), || Ok(PathBuf::from("/normal"))).unwrap();
        assert_eq!(got, PathBuf::from("/normal"));
    }

    #[test]
    fn default_error_is_passed_through() {
        let got = resolve_local_data_dir::<String>(None, || Err("없음".to_string()));
        assert_eq!(got, Err("없음".to_string()));
    }
}
