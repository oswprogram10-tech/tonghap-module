//! 번들된 llama.cpp·whisper.cpp 실행 파일로 속도를 잰다.
//! 엔진 파일은 scripts/fetch-engines.mjs가 src-tauri/resources/engines/에 넣는다.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::{model_path, model_spec, DiagResult, ModelKind};
use crate::engine::{engine, exe, resources, run, tail};

pub const LLM_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const STT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// resources/samples/jfk.wav 길이(초)
const SAMPLE_SECONDS: f64 = 11.0;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub llama_bench: bool,
    pub llama_server: bool,
    pub whisper_cli: bool,
    /// fetch-engines가 남긴 버전 정보(VERSIONS.json)
    pub versions: Option<serde_json::Value>,
}

#[tauri::command]
pub fn diag_engines(app: AppHandle) -> DiagResult<EngineInfo> {
    let root = resources(&app)?.join("engines");
    Ok(EngineInfo {
        llama_bench: root.join("llama").join(exe("llama-bench")).exists(),
        llama_server: root.join("llama").join(exe("llama-server")).exists(),
        whisper_cli: root.join("whisper").join(exe("whisper-cli")).exists(),
        versions: std::fs::read_to_string(root.join("VERSIONS.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

#[derive(Deserialize)]
pub struct LlamaBenchRow {
    pub n_prompt: u64,
    pub n_gen: u64,
    pub avg_ts: f64,
    #[serde(default)]
    backends: String,
    #[serde(default)]
    gpu_info: String,
    #[serde(default)]
    cpu_info: String,
    #[serde(default)]
    n_threads: u64,
    #[serde(default)]
    n_gpu_layers: i64,
}

/// llama-bench `-o json` 출력에서 프롬프트 처리(pp)와 토큰 생성(tg) 행을 찾는다.
pub fn parse_llama_bench(stdout: &str) -> DiagResult<(LlamaBenchRow, LlamaBenchRow)> {
    let rows: Vec<LlamaBenchRow> =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("측정 결과를 읽을 수 없음: {e}"))?;
    let mut pp = None;
    let mut tg = None;
    for r in rows {
        if r.n_prompt > 0 && r.n_gen == 0 && pp.is_none() {
            pp = Some(r);
        } else if r.n_gen > 0 && r.n_prompt == 0 && tg.is_none() {
            tg = Some(r);
        }
    }
    match (pp, tg) {
        (Some(pp), Some(tg)) => Ok((pp, tg)),
        _ => Err("측정 결과에 프롬프트/생성 항목이 없습니다".to_string()),
    }
}

/// llama-bench 인자. mode: "gpu"(가능한 모든 층을 GPU에) 또는 "cpu"(GPU 장치 사용 안 함)
pub fn llama_bench_args(model: &Path, mode: &str) -> DiagResult<Vec<String>> {
    let mut args: Vec<String> = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        // 평어 생성과 비슷하게: 프롬프트 처리 512토큰, 생성 128토큰, 2회 반복
        "-p".into(),
        "512".into(),
        "-n".into(),
        "128".into(),
        "-r".into(),
        "2".into(),
        "-o".into(),
        "json".into(),
    ];
    match mode {
        "gpu" => args.extend(["-ngl".into(), "99".into()]),
        "cpu" => args.extend(["-ngl".into(), "0".into(), "-dev".into(), "none".into()]),
        _ => return Err(format!("알 수 없는 측정 방식: {mode}")),
    }
    Ok(args)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmBench {
    pub model_id: String,
    pub model_bytes: u64,
    pub mode: String,
    pub prompt_tokens_per_sec: f64,
    pub gen_tokens_per_sec: f64,
    pub backends: String,
    pub gpu_info: String,
    pub cpu_info: String,
    pub threads: u64,
    pub gpu_layers: i64,
    pub elapsed_ms: u128,
}

/// mode: "gpu"(가능한 모든 층을 GPU에) 또는 "cpu"(GPU 장치 사용 안 함)
#[tauri::command]
pub async fn diag_bench_llm(app: AppHandle, model_id: String, mode: String) -> DiagResult<LlmBench> {
    let spec = model_spec(&model_id)?;
    if spec.kind != ModelKind::Llm {
        return Err("언어 모델이 아닙니다".to_string());
    }
    let model = model_path(&app, spec)?;
    if !model.exists() {
        return Err("모델 파일이 준비되지 않았습니다".to_string());
    }
    let program = engine(&app, "llama", "llama-bench")?;

    let args = llama_bench_args(&model, &mode)?;

    let out = tauri::async_runtime::spawn_blocking(move || run(&program, &args, LLM_TIMEOUT))
        .await
        .map_err(|e| e.to_string())??;

    let (pp, tg) = parse_llama_bench(&out.stdout).map_err(|e| format!("{e}\n{}", tail(&out.stderr, 800)))?;

    Ok(LlmBench {
        model_id,
        model_bytes: spec.size,
        mode,
        prompt_tokens_per_sec: pp.avg_ts,
        gen_tokens_per_sec: tg.avg_ts,
        backends: tg.backends,
        gpu_info: tg.gpu_info,
        cpu_info: tg.cpu_info,
        threads: tg.n_threads,
        gpu_layers: tg.n_gpu_layers,
        elapsed_ms: out.elapsed.as_millis(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SttBench {
    pub model_id: String,
    pub audio_seconds: f64,
    pub elapsed_ms: u128,
    /// 처리 시간 ÷ 음성 길이 (1보다 작을수록 빠름)
    pub real_time_factor: f64,
    pub threads: usize,
    pub transcript: String,
}

#[tauri::command]
pub async fn diag_bench_whisper(app: AppHandle, model_id: String) -> DiagResult<SttBench> {
    let spec = model_spec(&model_id)?;
    if spec.kind != ModelKind::Stt {
        return Err("음성 인식 모델이 아닙니다".to_string());
    }
    let model = model_path(&app, spec)?;
    if !model.exists() {
        return Err("모델 파일이 준비되지 않았습니다".to_string());
    }
    let program = engine(&app, "whisper", "whisper-cli")?;
    let sample = resources(&app)?.join("samples").join("jfk.wav");
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);

    // 모델 불러오기 시간까지 포함한 실제 사용 시간을 잰다.
    let args: Vec<String> = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        sample.to_string_lossy().into_owned(),
        "-l".into(),
        "en".into(),
        "-t".into(),
        threads.to_string(),
        "-nt".into(),
        "-np".into(),
    ];
    let out = tauri::async_runtime::spawn_blocking(move || run(&program, &args, STT_TIMEOUT))
        .await
        .map_err(|e| e.to_string())??;

    let secs = out.elapsed.as_secs_f64();
    Ok(SttBench {
        model_id,
        audio_seconds: SAMPLE_SECONDS,
        elapsed_ms: out.elapsed.as_millis(),
        real_time_factor: secs / SAMPLE_SECONDS,
        threads,
        transcript: out.stdout.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// llama-bench json_printer 형식(tools/llama-bench/llama-bench.cpp, b11600)을 줄인 예시.
    const SAMPLE: &str = r#"[
  {
    "build_commit": "abc1234",
    "build_number": 11600,
    "cpu_info": "Intel(R) Core(TM) i5-1235U",
    "gpu_info": "Intel(R) Iris(R) Xe Graphics",
    "backends": "Vulkan",
    "model_filename": "qwen2.5-0.5b-instruct-q4_k_m.gguf",
    "model_size": 491400032,
    "n_threads": 4,
    "n_gpu_layers": 99,
    "no_kv_offload": false,
    "devices": "auto",
    "n_prompt": 512,
    "n_gen": 0,
    "n_depth": 0,
    "test_time": "2026-11-03T20:00:00Z",
    "avg_ns": 812345678,
    "stddev_ns": 1234,
    "avg_ts": 630.271,
    "stddev_ts": 1.5,
    "samples_ns": [ 812345678, 812345678 ],
    "samples_ts": [ 630.2, 630.3 ]
  },
  {
    "build_commit": "abc1234",
    "build_number": 11600,
    "cpu_info": "Intel(R) Core(TM) i5-1235U",
    "gpu_info": "Intel(R) Iris(R) Xe Graphics",
    "backends": "Vulkan",
    "model_size": 491400032,
    "n_threads": 4,
    "n_gpu_layers": 99,
    "n_prompt": 0,
    "n_gen": 128,
    "n_depth": 0,
    "avg_ns": 2000000000,
    "avg_ts": 64.0,
    "stddev_ts": 0.4,
    "samples_ns": [ 2000000000, 2000000000 ],
    "samples_ts": [ 64.0, 64.0 ]
  }
]
"#;

    #[test]
    fn parses_prompt_and_generation_rows() {
        let (pp, tg) = parse_llama_bench(SAMPLE).unwrap();
        assert_eq!(pp.n_prompt, 512);
        assert!((pp.avg_ts - 630.271).abs() < 1e-9);
        assert_eq!(tg.n_gen, 128);
        assert!((tg.avg_ts - 64.0).abs() < 1e-9);
        assert_eq!(tg.backends, "Vulkan");
        assert_eq!(tg.n_gpu_layers, 99);
        assert_eq!(tg.n_threads, 4);
    }

    #[test]
    fn rejects_missing_rows_and_garbage() {
        assert!(parse_llama_bench("[]").is_err());
        assert!(parse_llama_bench("not json").is_err());
    }
}
