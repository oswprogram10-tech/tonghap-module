//! 번들된 llama.cpp·whisper.cpp 실행 파일로 속도를 잰다.
//! 엔진 파일은 scripts/fetch-engines.mjs가 src-tauri/resources/engines/에 넣는다.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::{model_path, model_spec, DiagResult, ModelKind};

const LLM_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const STT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// resources/samples/jfk.wav 길이(초)
const SAMPLE_SECONDS: f64 = 11.0;

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn resources(app: &AppHandle) -> DiagResult<PathBuf> {
    Ok(app
        .path()
        .resource_dir()
        .map_err(|e| format!("리소스 폴더를 찾을 수 없음: {e}"))?
        .join("resources"))
}

fn engine(app: &AppHandle, dir: &str, name: &str) -> DiagResult<PathBuf> {
    let path = resources(app)?.join("engines").join(dir).join(exe(name));
    if !path.exists() {
        return Err(format!(
            "엔진 파일이 없습니다({}). 개발 환경이라면 'npm run fetch:engines'를 실행하세요",
            path.display()
        ));
    }
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub llama_bench: bool,
    pub whisper_cli: bool,
    /// fetch-engines가 남긴 버전 정보(VERSIONS.json)
    pub versions: Option<serde_json::Value>,
}

#[tauri::command]
pub fn diag_engines(app: AppHandle) -> DiagResult<EngineInfo> {
    let root = resources(&app)?.join("engines");
    Ok(EngineInfo {
        llama_bench: root.join("llama").join(exe("llama-bench")).exists(),
        whisper_cli: root.join("whisper").join(exe("whisper-cli")).exists(),
        versions: std::fs::read_to_string(root.join("VERSIONS.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

struct RunOutput {
    stdout: String,
    stderr: String,
    elapsed: Duration,
}

/// 콘솔 창 없이 실행하고, 시간이 지나면 강제 종료한다.
fn run(program: &Path, args: &[String], timeout: Duration) -> DiagResult<RunOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        // DLL·백엔드 모듈을 실행 파일 옆에서 찾도록 작업 폴더를 맞춘다.
        .current_dir(program.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("실행 실패({}): {e}", program.display()))?;

    // 출력이 파이프 버퍼를 채워 멈추지 않도록 별도 스레드에서 읽는다.
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut s);
        s
    });

    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{}초 안에 끝나지 않아 중단했습니다", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let elapsed = started.elapsed();
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();

    if !status.success() {
        return Err(format!("종료 코드 {:?}\n{}", status.code(), tail(&stderr, 1500)));
    }
    Ok(RunOutput { stdout, stderr, elapsed })
}

fn tail(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

#[derive(Deserialize)]
struct LlamaBenchRow {
    n_prompt: u64,
    n_gen: u64,
    avg_ts: f64,
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
fn parse_llama_bench(stdout: &str) -> DiagResult<(LlamaBenchRow, LlamaBenchRow)> {
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
    match mode.as_str() {
        "gpu" => args.extend(["-ngl".into(), "99".into()]),
        "cpu" => args.extend(["-ngl".into(), "0".into(), "-dev".into(), "none".into()]),
        _ => return Err(format!("알 수 없는 측정 방식: {mode}")),
    }

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

    #[test]
    fn tail_keeps_last_chars() {
        assert_eq!(tail("가나다라마", 2), "라마");
        assert_eq!(tail("ab", 10), "ab");
    }
}
