//! 엔진 통합 테스트: 앱이 쓰는 실제 인자·요청 본문으로 번들 엔진(whisper-cli, llama-bench, llama-server)이 동작하는지 본다.
//! 설계: `docs/design/12-windows-integration-tests.md` 1절.
//!
//! 엔진은 윈도우용 실행 파일뿐이라 모두 `#[ignore]`다. 실행:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test engines -- --ignored --nocapture --test-threads=1
//!
//! 환경 변수
//! - `TONGHAP_ENGINES_DIR`: `llama/`, `whisper/`를 품은 폴더(`src-tauri/resources/engines`). 그 옆의 `samples/jfk.wav`를 쓴다.
//! - `TONGHAP_TEST_WHISPER_MODEL`: whisper 모델 파일(ggml-tiny-q5_1.bin)
//! - `TONGHAP_TEST_LLM_MODEL`: 언어 모델 파일(Qwen3-0.6B-Q8_0.gguf)
//! - `TONGHAP_TEST_LOG_DIR`(선택): llama-server 로그를 둘 폴더. 없으면 임시 폴더.
//! - `TONGHAP_REQUIRE_ENGINES=1`(선택): 위 필수 변수가 없으면 건너뛰지 않고 실패(워크플로가 쓴다).
//!
//! 필수 변수가 없고 `TONGHAP_REQUIRE_ENGINES`도 없으면 안내만 출력하고 통과한다(리눅스 개발 환경).

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::Value;
use tonghap_module_lib::testing::*;

struct Env {
    engines: PathBuf,
    whisper_model: PathBuf,
    llm_model: PathBuf,
}

impl Env {
    /// `resources/` 폴더(= engines 폴더의 부모). `engine_in`과 샘플 경로에 쓴다.
    fn resources(&self) -> PathBuf {
        self.engines.parent().expect("엔진 폴더의 부모").to_path_buf()
    }

    fn whisper_cli(&self) -> PathBuf {
        engine_in(&self.resources(), "whisper", "whisper-cli").unwrap_or_else(|e| panic!("{e}"))
    }

    fn sample(&self) -> PathBuf {
        let p = self.resources().join("samples").join("jfk.wav");
        assert!(p.exists(), "샘플 음성이 없습니다: {}", p.display());
        p
    }
}

/// 엔진은 실행 파일 옆을 작업 폴더로 실행되므로 상대 경로가 어긋나지 않게 절대 경로로 만든다.
/// `canonicalize`는 윈도우에서 `\\?\` 접두를 붙여 엔진이 못 읽을 수 있어 쓰지 않는다.
fn abs(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(|v| abs(Path::new(&v)))
}

/// 필요한 환경 변수가 있으면 `Some`. 없으면 건너뛰고(`None`), `TONGHAP_REQUIRE_ENGINES`가 켜져 있으면 실패한다.
fn setup(test: &str, need_whisper: bool, need_llm: bool) -> Option<Env> {
    let engines = env_path("TONGHAP_ENGINES_DIR");
    let whisper_model = env_path("TONGHAP_TEST_WHISPER_MODEL");
    let llm_model = env_path("TONGHAP_TEST_LLM_MODEL");

    let mut missing: Vec<&str> = Vec::new();
    if engines.is_none() {
        missing.push("TONGHAP_ENGINES_DIR");
    }
    if need_whisper && whisper_model.is_none() {
        missing.push("TONGHAP_TEST_WHISPER_MODEL");
    }
    if need_llm && llm_model.is_none() {
        missing.push("TONGHAP_TEST_LLM_MODEL");
    }
    if !missing.is_empty() {
        let msg = format!("{test}: 환경 변수가 없습니다: {}", missing.join(", "));
        if std::env::var("TONGHAP_REQUIRE_ENGINES").is_ok_and(|v| v == "1") {
            panic!("{msg}");
        }
        println!("건너뜀 - {msg} (엔진은 윈도우에서 'npm run fetch:engines'로 받는다)");
        return None;
    }
    let env = Env { engines: engines.unwrap(), whisper_model: whisper_model.unwrap_or_default(), llm_model: llm_model.unwrap_or_default() };
    assert!(env.engines.is_dir(), "엔진 폴더가 없습니다: {}", env.engines.display());
    if need_whisper {
        assert!(env.whisper_model.is_file(), "whisper 모델이 없습니다: {}", env.whisper_model.display());
    }
    if need_llm {
        assert!(env.llm_model.is_file(), "언어 모델이 없습니다: {}", env.llm_model.display());
    }
    Some(env)
}

/// 16kHz 모노 16비트 WAV(앱 화면이 만드는 형식). 샘플은 모두 0(무음).
fn silent_wav(seconds: u32) -> Vec<u8> {
    let rate: u32 = 16_000;
    let data_len = (rate * seconds * 2) as usize;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // 모노
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    v.resize(44 + data_len, 0);
    v
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tonghap-engines-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 폴더");
    dir
}

// ---------- 환경 변수 없이도 도는 확인 ----------

#[test]
fn silent_wav_passes_app_validation() {
    let seconds = validate_wav(&silent_wav(1)).expect("validate_wav");
    assert!((seconds - 1.0).abs() < 0.01, "길이 {seconds}");
    assert_eq!(clean_transcript("[BLANK_AUDIO]\n (silence) \n").unwrap_err(), "말소리를 찾지 못했습니다");
    assert!(llama_bench_args(Path::new("m.gguf"), "??").is_err());
}

// ---------- whisper-cli ----------

#[test]
#[ignore = "윈도우 엔진과 모델이 필요함(환경 변수 TONGHAP_*)"]
fn whisper_transcribes_sample_with_app_args() {
    let Some(env) = setup("whisper_transcribes_sample_with_app_args", true, false) else { return };
    let program = env.whisper_cli();
    let sample = env.sample();

    // 앱의 자막 실행(-l ko): 영어 음성이라 내용은 보지 않고, 성공 종료와 비어 있지 않은 결과만 본다.
    let args = whisper_args(&env.whisper_model, &sample, "ko");
    assert!(args.windows(2).any(|w| w == ["-l", "ko"]), "앱 인자에 -l ko가 있어야 함: {args:?}");
    let out = run(&program, &args, STT_TIMEOUT).unwrap_or_else(|e| panic!("whisper-cli(ko) 실패: {e}"));
    println!("ko ({} ms): {}", out.elapsed.as_millis(), out.stdout.trim());
    let cleaned = clean_transcript(&out.stdout).unwrap_or_else(|e| panic!("ko 결과가 비어 있음({e}). stderr 끝: {}", tail(&out.stderr, 1500)));
    assert!(!cleaned.trim().is_empty());

    // 같은 인자 목록에 언어만 en: 엔진 자체가 소리를 알아듣는지.
    let args = whisper_args(&env.whisper_model, &sample, "en");
    let out = run(&program, &args, STT_TIMEOUT).unwrap_or_else(|e| panic!("whisper-cli(en) 실패: {e}"));
    println!("en ({} ms): {}", out.elapsed.as_millis(), out.stdout.trim());
    let text = clean_transcript(&out.stdout).unwrap_or_else(|e| panic!("en 결과가 비어 있음({e}). stderr 끝: {}", tail(&out.stderr, 1500)));
    assert!(text.to_lowercase().contains("fellow americans"), "jfk.wav 받아쓰기에 'fellow Americans'가 없음: {text}");
}

#[test]
#[ignore = "윈도우 엔진과 모델이 필요함(환경 변수 TONGHAP_*)"]
fn whisper_silence_goes_through_empty_result_path() {
    let Some(env) = setup("whisper_silence_goes_through_empty_result_path", true, false) else { return };
    let program = env.whisper_cli();

    // 앱 화면이 만드는 것과 같은 모양의 WAV가 Rust 쪽 검사를 통과해야 한다.
    let wav = silent_wav(1);
    validate_wav(&wav).unwrap_or_else(|e| panic!("validate_wav가 거부함: {e}"));

    let dir = scratch_dir("silence");
    let path = dir.join("silence.wav");
    std::fs::write(&path, &wav).unwrap();
    let args = whisper_args(&env.whisper_model, &path, "ko");
    let out = run(&program, &args, STT_TIMEOUT).unwrap_or_else(|e| panic!("whisper-cli(무음) 실패: {e}"));
    println!("무음 결과: {:?}", out.stdout.trim());
    match clean_transcript(&out.stdout) {
        Err(e) => assert_eq!(e, "말소리를 찾지 못했습니다"),
        // 작은 모델은 무음에서 문장을 지어낼 때가 있다. 엔진 오류가 아니므로 알리기만 한다.
        Ok(text) => println!("주의: tiny 모델이 무음에서 문장을 만들어 냄(환각): {text:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- llama-bench ----------

#[test]
#[ignore = "윈도우 엔진과 모델이 필요함(환경 변수 TONGHAP_*)"]
fn llama_bench_runs_with_app_args() {
    let Some(env) = setup("llama_bench_runs_with_app_args", false, true) else { return };
    let program = engine_in(&env.resources(), "llama", "llama-bench").unwrap_or_else(|e| panic!("{e}"));

    // CPU 측정(GPU 장치 사용 안 함)은 어느 러너에서나 되어야 한다.
    let args = llama_bench_args(&env.llm_model, "cpu").expect("cpu 인자");
    let out = run(&program, &args, LLM_TIMEOUT).unwrap_or_else(|e| panic!("llama-bench(cpu) 실패: {e}"));
    let (pp, tg) = parse_llama_bench(&out.stdout).unwrap_or_else(|e| panic!("{e}\nstdout: {}\nstderr 끝: {}", tail(&out.stdout, 800), tail(&out.stderr, 800)));
    println!("cpu: 프롬프트 {:.1} t/s, 생성 {:.1} t/s ({} ms)", pp.avg_ts, tg.avg_ts, out.elapsed.as_millis());
    assert!(pp.avg_ts > 0.0 && tg.avg_ts > 0.0);

    // GPU 측정: 러너에는 GPU가 없을 수 있어 실패해도 알리기만 한다(앱도 CPU로 물러선다).
    let args = llama_bench_args(&env.llm_model, "gpu").expect("gpu 인자");
    match run(&program, &args, LLM_TIMEOUT).and_then(|o| parse_llama_bench(&o.stdout)) {
        Ok((pp, tg)) => println!("gpu: 프롬프트 {:.1} t/s, 생성 {:.1} t/s", pp.avg_ts, tg.avg_ts),
        Err(e) => println!("주의: GPU 측정을 못 함(GPU 없는 러너일 수 있음): {}", tail(&e, 500)),
    }
}

// ---------- llama-server ----------

fn synthetic_bundle() -> Bundle {
    let obs = |id: &str, date: &str, tag: &str, text: &str, quote: Option<&str>| Evidence::Observation {
        id: id.into(),
        date: date.into(),
        topic: Some("학교".into()),
        standards: vec!["[2슬01-01]".into()],
        tags: vec![tag.into()],
        text: text.into(),
        quote: quote.map(String::from),
        together: false,
    };
    Bundle {
        student: BundleStudent { alias: "학생".into() },
        subject: "슬".into(),
        period: BundlePeriod { from: "2027-03-02".into(), to: "2027-07-31".into() },
        evidence: vec![
            obs("o:t1", "2027-04-12", "탐구", "돋보기로 교실 구석의 곤충을 오래 관찰하고 발견한 점을 그림으로 그림", None),
            obs("o:t2", "2027-04-20", "질문", "학교 계단 난간이 왜 있는지 궁금해하며 선생님께 질문함", Some("계단에 이거는 왜 있어요?")),
            obs("o:t3", "2027-05-03", "협력", "친구와 학교 안 안전 표지판을 찾아 사진으로 정리함", None),
            Evidence::Habit { id: "h:t4".into(), title: "인사하기".into(), done_days: 18, school_days: 20 },
            Evidence::Session {
                id: "s:t5".into(),
                date: "2027-05-10".into(),
                topic: Some("학교".into()),
                title: "학교 안전 지도 만들기".into(),
                standards: vec!["[2슬01-01]".into()],
            },
        ],
        standards_text: [("[2슬01-01]".to_string(), "학교 안팎의 모습과 생활을 탐색하며 안전한 학교 생활을 한다.".to_string())].into_iter().collect(),
    }
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(500)).is_ok()
}

#[test]
#[ignore = "윈도우 엔진과 모델이 필요함(환경 변수 TONGHAP_*)"]
fn llama_server_answers_app_request_with_evidence_ids() {
    let Some(env) = setup("llama_server_answers_app_request_with_evidence_ids", false, true) else { return };
    let program = engine_in(&env.resources(), "llama", "llama-server").unwrap_or_else(|e| panic!("{e}"));
    let log_dir = env_path("TONGHAP_TEST_LOG_DIR").unwrap_or_else(|| scratch_dir("server-log"));
    let log = log_dir.join("llama-server.log");
    let log_tail = || std::fs::read_to_string(&log).map(|s| tail(&s, 3000)).unwrap_or_else(|_| "(로그 없음)".into());

    // 앱의 서버 관리 코드로 시작(GPU 시도 -> CPU 대체 포함). /health가 200이 될 때까지 기다린다.
    let server = LlmServer::real();
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let port = match server.ensure(&program, &env.llm_model, Some(log.clone()), &cancel) {
        Ok(p) => p,
        Err(e) => panic!("llama-server 시작 실패: {}\n--- 서버 로그 끝 ---\n{}", e.message(), log_tail()),
    };
    println!("llama-server 준비: 포트 {port}, {} ms, 로그 {}", started.elapsed().as_millis(), log.display());
    assert!(server.is_running());
    let (status, body) = http(port, "GET", "/health", None, Duration::from_secs(5), &cancel).expect("/health");
    assert_eq!(status, 200, "/health: {body}");

    // 앱의 요청 본문(JSON 스키마의 근거 id enum, enable_thinking:false)을 합성 묶음으로 만들어 보낸다.
    let bundle = synthetic_bundle();
    let ids: Vec<String> = bundle.evidence.iter().map(|e| e.id().to_string()).collect();
    assert_eq!(ids.len(), 5);
    let mut last = String::new();
    let mut ok = false;
    // 앱처럼 seed를 바꿔 최대 3번까지 시도한다(작은 모델이 길게 늘어져 잘릴 수 있음).
    for attempt in 0..3u32 {
        let req = request_body(&bundle, Style::Ham, 250, 1234 + attempt);
        assert_eq!(req["chat_template_kwargs"]["enable_thinking"], Value::Bool(false));
        let enum_len = req["response_format"]["json_schema"]["schema"]["properties"]["sentences"]["items"]["properties"]["evidence"]["items"]["enum"].as_array().map(Vec::len);
        assert_eq!(enum_len, Some(5));
        let raw = match chat_once(port, &req, &cancel) {
            Ok(r) => r,
            Err(e) => {
                last = format!("요청 실패: {}", e.message());
                continue;
            }
        };
        println!("응답({attempt}): {raw}");
        assert!(!raw.contains("<think>") && !raw.contains("</think>"), "<think>가 섞임: {raw}");
        let v: Value = match serde_json::from_str(raw.trim()) {
            Ok(v) => v,
            Err(e) => {
                last = format!("JSON이 아님({e}): {raw}");
                continue;
            }
        };
        let sentences = v["sentences"].as_array().cloned().unwrap_or_default();
        assert!(!sentences.is_empty(), "sentences가 비어 있음: {raw}");
        for s in &sentences {
            assert!(s["text"].as_str().is_some_and(|t| !t.trim().is_empty()), "빈 문장: {raw}");
            let ev = s["evidence"].as_array().unwrap_or_else(|| panic!("evidence가 배열이 아님: {raw}"));
            assert!(!ev.is_empty(), "근거가 없는 문장: {raw}");
            for e in ev {
                let id = e.as_str().unwrap_or_default();
                assert!(ids.iter().any(|x| x == id), "묶음에 없는 근거 id '{id}': {raw}");
            }
        }
        ok = true;
        break;
    }
    if !ok {
        panic!("3번 시도해도 쓸 수 있는 JSON 응답을 받지 못함: {last}\n--- 서버 로그 끝 ---\n{}", log_tail());
    }

    // 서버 종료와 프로세스 정리: 포트가 닫혀야 한다.
    server.stop();
    assert!(!server.is_running());
    let deadline = Instant::now() + Duration::from_secs(10);
    while port_open(port) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(!port_open(port), "stop() 뒤에도 포트 {port}가 열려 있음(프로세스가 남음)");
}
