//! 기기 점검 도구: 학교 PC에서 로컬 AI가 돌아갈 수 있는지 측정한다.
//!
//! - 시스템 정보(OS, CPU, RAM, 디스크, WebView2)와 Vulkan GPU 목록
//! - 측정용 모델 다운로드(이어받기, SHA-256 확인) 또는 직접 선택
//! - llama.cpp(llama-bench)·whisper.cpp(whisper-cli) 속도 측정
//! - 결과를 문서 폴더에 JSON·텍스트로 저장

pub mod bench;
pub mod download;
pub mod system;
mod vulkan;

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// 진단 명령의 오류는 화면에 그대로 보여 줄 한국어 문자열로 돌려준다.
pub type DiagResult<T> = Result<T, String>;

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Llm,
    Stt,
}

/// 측정용 모델 목록. 해시는 Hugging Face LFS의 SHA-256.
pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ModelKind,
    pub file_name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub license: &'static str,
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "qwen2.5-0.5b-q4km",
        label: "언어 모델 · 빠른 점검 (Qwen2.5 0.5B, Q4_K_M)",
        kind: ModelKind::Llm,
        file_name: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf",
        size: 491_400_032,
        sha256: "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db",
        license: "Apache-2.0",
    },
    ModelSpec {
        id: "qwen2.5-1.5b-q4km",
        label: "언어 모델 · 정밀 점검 (Qwen2.5 1.5B, Q4_K_M)",
        kind: ModelKind::Llm,
        file_name: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf",
        size: 1_117_320_736,
        sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
        license: "Apache-2.0",
    },
    ModelSpec {
        id: "whisper-base",
        label: "음성 인식 (Whisper base)",
        kind: ModelKind::Stt,
        file_name: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        size: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        license: "MIT",
    },
];

pub fn model_spec(id: &str) -> DiagResult<&'static ModelSpec> {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("알 수 없는 모델: {id}"))
}

pub fn models_dir(app: &AppHandle) -> DiagResult<PathBuf> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("앱 데이터 폴더를 찾을 수 없음: {e}"))?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| format!("모델 폴더를 만들 수 없음({}): {e}", dir.display()))?;
    Ok(dir)
}

pub fn model_path(app: &AppHandle, spec: &ModelSpec) -> DiagResult<PathBuf> {
    Ok(models_dir(app)?.join(spec.file_name))
}

/// 점검 결과를 문서 폴더 아래 `바슬즐 기기점검`에 저장하고 JSON 파일 경로를 돌려준다.
#[tauri::command]
pub async fn diag_save_report(app: AppHandle, report: serde_json::Value, summary: String) -> DiagResult<String> {
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().desktop_dir())
        .map_err(|e| format!("문서 폴더를 찾을 수 없음: {e}"))?
        .join("바슬즐 기기점검");
    std::fs::create_dir_all(&base).map_err(|e| format!("저장 폴더를 만들 수 없음({}): {e}", base.display()))?;

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let json_path = base.join(format!("기기점검-{stamp}.json"));
    let txt_path = base.join(format!("기기점검-{stamp}.txt"));

    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&json_path, json).map_err(|e| format!("결과 저장 실패({}): {e}", json_path.display()))?;
    // 메모장에서 한글이 깨지지 않도록 UTF-8 BOM과 CRLF를 쓴다.
    let txt = format!("\u{feff}{}", summary.replace("\r\n", "\n").replace('\n', "\r\n"));
    std::fs::write(&txt_path, txt).map_err(|e| format!("결과 저장 실패({}): {e}", txt_path.display()))?;

    Ok(json_path.to_string_lossy().into_owned())
}
