//! 평어 초안용 언어 모델 서버(llama-server) 관리. 설계: docs/design/09-local-llm-drafts.md
//!
//! - 처음 필요할 때 `127.0.0.1`의 빈 포트로 띄우고 `/health`가 준비될 때까지 기다린다.
//! - 마지막 사용 뒤 5분이 지나면 끄고(`sweep`), 앱이 끝날 때도 끈다(`shutdown`).
//! - GPU로 못 띄우면(시작 실패·충돌) `-ngl 0`으로 한 번 더 띄운다.
//! - 프로세스 시작·상태 확인·포트 고르기는 주입할 수 있어 가짜로 테스트한다.
//! - 요청은 표준 라이브러리 TCP로 `127.0.0.1`에만 보낸다(바깥 주소로는 연결할 수 없는 구조).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::AppHandle;

use crate::engine::{hidden_command, tail};

pub const HOST: &str = "127.0.0.1";
pub const IDLE_LIMIT: Duration = Duration::from_secs(5 * 60);
/// 모델을 불러오는 데 기다리는 시간(느린 디스크에서 2.5GB를 읽는 경우).
const READY_TIMEOUT: Duration = Duration::from_secs(240);
const READY_POLL: Duration = Duration::from_millis(500);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(20);
pub const CTX_SIZE: u32 = 8192;
pub const GPU_LAYERS: u32 = 99;

pub const CANCELLED_MESSAGE: &str = "초안 만들기를 취소했습니다";

#[derive(Debug, Clone, PartialEq)]
pub enum LlmError {
    Cancelled,
    /// 요청 하나가 실패함(다시 시도해 볼 수 있다).
    Failed(String),
    /// 서버를 띄울 수 없음(다시 시도해도 같다).
    Unavailable(String),
}

impl From<String> for LlmError {
    fn from(s: String) -> Self {
        LlmError::Failed(s)
    }
}

impl LlmError {
    pub fn message(&self) -> String {
        match self {
            LlmError::Cancelled => CANCELLED_MESSAGE.to_string(),
            LlmError::Failed(m) | LlmError::Unavailable(m) => m.clone(),
        }
    }
}

// ---------- 실행 인자 ----------

/// llama-server 인자. 바깥 주소로는 열지 않도록 `--host 127.0.0.1`을 항상 넣는다.
/// `--reasoning off`: Qwen3의 생각(thinking)을 서버 기본값으로 끈다(요청에서도 `enable_thinking: false`를 보낸다).
/// `-np 1`: 슬롯을 하나로 해 8192 토큰 문맥을 나눠 쓰지 않는다.
pub fn server_args(model: &Path, port: u16, gpu_layers: u32) -> Vec<String> {
    vec![
        "--host".into(),
        HOST.into(),
        "--port".into(),
        port.to_string(),
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-c".into(),
        CTX_SIZE.to_string(),
        "-ngl".into(),
        gpu_layers.to_string(),
        "-np".into(),
        "1".into(),
        "--jinja".into(),
        "--reasoning".into(),
        "off".into(),
        "--no-webui".into(),
    ]
}

#[cfg_attr(not(test), allow(dead_code))]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub port: u16,
    pub gpu_layers: u32,
    /// 서버가 stderr에 쓰는 로그(시작 실패 원인 확인용).
    pub log: Option<PathBuf>,
}

pub trait Proc: Send {
    fn alive(&mut self) -> bool;
    fn kill(&mut self);
}

impl Proc for Child {
    fn alive(&mut self) -> bool {
        matches!(self.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = Child::kill(self);
        let _ = self.wait();
    }
}

type Launch = Box<dyn Fn(&LaunchSpec) -> Result<Box<dyn Proc>, String> + Send + Sync>;
type Probe = Box<dyn Fn(u16) -> bool + Send + Sync>;
type PickPort = Box<dyn Fn() -> Result<u16, String> + Send + Sync>;

struct Running {
    proc: Box<dyn Proc>,
    port: u16,
    model: PathBuf,
}

struct Inner {
    running: Option<Running>,
    last_used: Instant,
    active: usize,
}

pub struct LlmServer {
    inner: Mutex<Inner>,
    launch: Launch,
    probe: Probe,
    pick_port: PickPort,
    ready_timeout: Duration,
    ready_poll: Duration,
    idle: Duration,
    closing: AtomicBool,
    /// 화면의 「취소」. 작업을 시작할 때 내린다.
    pub cancel: AtomicBool,
    /// 초안은 한 번에 하나씩만 만든다.
    pub one_at_a_time: Mutex<()>,
}

/// 사용하는 동안 유휴 종료를 막는 표시. 놓으면 마지막 사용 시각을 갱신한다.
pub struct ActiveGuard<'a>(&'a LlmServer);

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        let mut g = self.0.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.active = g.active.saturating_sub(1);
        g.last_used = Instant::now();
    }
}

impl LlmServer {
    pub fn new(launch: Launch, probe: Probe, pick_port: PickPort) -> LlmServer {
        LlmServer {
            inner: Mutex::new(Inner { running: None, last_used: Instant::now(), active: 0 }),
            launch,
            probe,
            pick_port,
            ready_timeout: READY_TIMEOUT,
            ready_poll: READY_POLL,
            idle: IDLE_LIMIT,
            closing: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            one_at_a_time: Mutex::new(()),
        }
    }

    /// 실제 프로세스·HTTP를 쓰는 서버.
    pub fn real() -> LlmServer {
        LlmServer::new(
            Box::new(|spec| {
                let mut cmd = hidden_command(&spec.program, &spec.args);
                cmd.stdout(Stdio::null());
                let log = spec.log.as_ref().and_then(|p| {
                    if let Some(dir) = p.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    std::fs::File::create(p).ok()
                });
                cmd.stderr(log.map_or_else(Stdio::null, Stdio::from));
                let child = cmd.spawn().map_err(|e| format!("언어 모델 서버를 실행할 수 없습니다({}): {e}", spec.program.display()))?;
                Ok(Box::new(child) as Box<dyn Proc>)
            }),
            Box::new(|port| matches!(http(port, "GET", "/health", None, Duration::from_secs(2), &AtomicBool::new(false)), Ok((200, _)))),
            Box::new(free_port),
        )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_running(&self) -> bool {
        self.lock().running.is_some()
    }

    #[cfg(test)]
    pub fn running_port(&self) -> Option<u16> {
        self.lock().running.as_ref().map(|r| r.port)
    }

    /// 사용 중 표시. 반환된 값을 쥐고 있는 동안은 유휴로 끄지 않는다.
    pub fn begin(&self) -> ActiveGuard<'_> {
        let mut g = self.lock();
        g.active += 1;
        g.last_used = Instant::now();
        ActiveGuard(self)
    }

    /// 서버를 띄워 두고(이미 같은 모델로 떠 있으면 그대로) 포트를 돌려준다.
    pub fn ensure(&self, program: &Path, model: &Path, log: Option<PathBuf>, cancel: &AtomicBool) -> Result<u16, LlmError> {
        let mut g = self.lock();
        let reuse = match g.running.as_mut() {
            Some(r) if r.model == model => r.proc.alive().then_some(r.port),
            _ => None,
        };
        if let Some(port) = reuse {
            g.last_used = Instant::now();
            return Ok(port);
        }
        // 다른 모델이거나 죽었으면 정리하고 새로 띄운다.
        if let Some(mut old) = g.running.take() {
            old.proc.kill();
        }
        let mut last_error = String::new();
        for gpu_layers in [GPU_LAYERS, 0] {
            if self.closing.load(Ordering::SeqCst) {
                return Err(LlmError::Unavailable("앱을 닫는 중입니다".to_string()));
            }
            let port = (self.pick_port)().map_err(LlmError::Unavailable)?;
            let spec = LaunchSpec { program: program.to_path_buf(), args: server_args(model, port, gpu_layers), port, gpu_layers, log: log.clone() };
            let mut proc = match (self.launch)(&spec) {
                Ok(p) => p,
                Err(e) => {
                    last_error = e;
                    continue;
                }
            };
            match self.wait_ready(proc.as_mut(), port, cancel) {
                Ok(()) => {
                    g.running = Some(Running { proc, port, model: model.to_path_buf() });
                    g.last_used = Instant::now();
                    return Ok(port);
                }
                Err(e) => {
                    proc.kill();
                    if e == LlmError::Cancelled {
                        return Err(e);
                    }
                    let log_tail = log.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| tail(s.trim(), 400)).unwrap_or_default();
                    last_error = match e {
                        LlmError::Failed(m) | LlmError::Unavailable(m) if log_tail.is_empty() => m,
                        LlmError::Failed(m) | LlmError::Unavailable(m) => format!("{m}\n{log_tail}"),
                        LlmError::Cancelled => String::new(),
                    };
                }
            }
        }
        Err(LlmError::Unavailable(format!("언어 모델 서버를 시작하지 못했습니다. {last_error}")))
    }

    fn wait_ready(&self, proc: &mut dyn Proc, port: u16, cancel: &AtomicBool) -> Result<(), LlmError> {
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(LlmError::Cancelled);
            }
            if self.closing.load(Ordering::SeqCst) {
                return Err(LlmError::Failed("앱을 닫는 중입니다".to_string()));
            }
            if !proc.alive() {
                return Err(LlmError::Failed("서버가 바로 종료되었습니다".to_string()));
            }
            if (self.probe)(port) {
                return Ok(());
            }
            if started.elapsed() > self.ready_timeout {
                return Err(LlmError::Failed(format!("{}초 안에 모델이 준비되지 않았습니다", self.ready_timeout.as_secs())));
            }
            std::thread::sleep(self.ready_poll);
        }
    }

    /// 쓰지 않은 지 `idle` 이상이고 사용 중이 아니면 끈다. 껐으면 true.
    pub fn sweep(&self, now: Instant) -> bool {
        let mut g = self.lock();
        if g.active > 0 || g.running.is_none() || now.saturating_duration_since(g.last_used) < self.idle {
            return false;
        }
        if let Some(mut r) = g.running.take() {
            r.proc.kill();
        }
        true
    }

    /// 떠 있는 서버를 끈다(앱 종료 때, 모델을 바꿀 때).
    pub fn stop(&self) {
        if let Some(mut r) = self.lock().running.take() {
            r.proc.kill();
        }
    }

    /// 앱 종료: 시작 중인 서버도 기다리지 않고 끊도록 표시한 뒤 끈다.
    pub fn shutdown(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
        self.stop();
    }
}

impl Drop for LlmServer {
    fn drop(&mut self) {
        if let Ok(g) = self.inner.get_mut() {
            if let Some(mut r) = g.running.take() {
                r.proc.kill();
            }
        }
    }
}

/// 유휴 종료 감시 스레드를 띄운다(서버가 사라지면 함께 끝난다).
pub fn spawn_sweeper(server: &Arc<LlmServer>) {
    let weak = Arc::downgrade(server);
    std::thread::spawn(move || loop {
        std::thread::sleep(SWEEP_EVERY);
        let Some(s) = weak.upgrade() else { return };
        if s.closing.load(Ordering::SeqCst) {
            return;
        }
        s.sweep(Instant::now());
    });
}

/// 앱 시작 때 호출: 서버 관리자를 관리 상태로 두고 감시 스레드를 띄운다.
pub fn start(app: &AppHandle) {
    use tauri::Manager;
    let server = Arc::new(LlmServer::real());
    spawn_sweeper(&server);
    app.manage(server);
}

/// 빈 포트를 하나 얻는다(127.0.0.1에서만 확인).
fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("빈 포트를 찾을 수 없습니다: {e}"))?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

// ---------- 127.0.0.1 HTTP ----------

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = find(data, b"\r\n").ok_or("응답 형식 오류(chunk)")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "응답 형식 오류(chunk)")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "응답 형식 오류(chunk)")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err("응답이 중간에 끊겼습니다".to_string());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

/// 응답 바이트에서 (상태 코드, 본문)을 꺼낸다.
pub fn parse_response(raw: &[u8]) -> Result<(u16, String), String> {
    let head_end = find(raw, b"\r\n\r\n").ok_or("응답 형식 오류")?;
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
    let status: u16 = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).ok_or("응답 형식 오류(상태 줄)")?;
    let body = &raw[head_end + 4..];
    let chunked = head.lines().any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.to_ascii_lowercase().contains("chunked"));
    let bytes = if chunked { decode_chunked(body)? } else { body.to_vec() };
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}

/// 127.0.0.1:`port`로 요청 한 번. `cancel`이 켜지면 연결을 끊고 돌아온다(서버는 연결이 끊기면 생성을 멈춘다).
pub fn http(port: u16, method: &str, path: &str, body: Option<&str>, timeout: Duration, cancel: &AtomicBool) -> Result<(u16, String), LlmError> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| LlmError::Failed(format!("서버에 연결할 수 없습니다: {e}")))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let payload = body.unwrap_or("");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {HOST}:{port}\r\nConnection: close\r\nAccept: application/json\r\n");
    if body.is_some() {
        req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", payload.len()));
    }
    req.push_str("\r\n");
    req.push_str(payload);
    stream.write_all(req.as_bytes()).map_err(|e| LlmError::Failed(format!("서버로 보내지 못했습니다: {e}")))?;

    let started = Instant::now();
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(LlmError::Cancelled);
        }
        if started.elapsed() > timeout {
            return Err(LlmError::Failed(format!("{}초 안에 응답이 오지 않았습니다", timeout.as_secs())));
        }
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => continue,
            Err(e) => return Err(LlmError::Failed(format!("응답을 읽지 못했습니다: {e}"))),
        }
    }
    parse_response(&raw).map_err(LlmError::Failed)
}

/// `/v1/chat/completions` 응답 본문에서 첫 메시지의 내용을 꺼낸다.
pub fn completion_content(body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("서버 응답을 읽을 수 없습니다: {e}"))?;
    v["choices"][0]["message"]["content"].as_str().map(str::to_string).ok_or_else(|| "서버 응답에 내용이 없습니다".to_string())
}

/// 채팅 요청 한 번(비스트리밍). 서버가 떠 있어야 한다.
pub fn chat_once(port: u16, request: &Value, cancel: &AtomicBool) -> Result<String, LlmError> {
    let (status, body) = http(port, "POST", "/v1/chat/completions", Some(&request.to_string()), REQUEST_TIMEOUT, cancel)?;
    if status != 200 {
        return Err(LlmError::Failed(format!("서버가 오류를 돌려주었습니다({status}): {}", tail(body.trim(), 300))));
    }
    completion_content(&body).map_err(LlmError::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// 가짜 프로세스: 살아 있는지와 kill 횟수를 바깥에서 본다.
    #[derive(Clone, Default)]
    struct Flags {
        alive: Arc<AtomicBool>,
        kills: Arc<AtomicUsize>,
    }

    struct FakeProc(Flags);

    impl Proc for FakeProc {
        fn alive(&mut self) -> bool {
            self.0.alive.load(Ordering::SeqCst)
        }
        fn kill(&mut self) {
            self.0.alive.store(false, Ordering::SeqCst);
            self.0.kills.fetch_add(1, Ordering::SeqCst);
        }
    }

    type Launched = Vec<(u16, u32, Vec<String>)>;

    struct Rig {
        server: LlmServer,
        specs: Arc<Mutex<Launched>>,
        procs: Arc<Mutex<Vec<Flags>>>,
    }

    /// `healthy`가 참인 gpu_layers 값일 때만 /health가 통과한다.
    fn rig(healthy_layers: Option<u32>) -> Rig {
        let specs = Arc::new(Mutex::new(Vec::new()));
        let procs = Arc::new(Mutex::new(Vec::new()));
        let layers_by_port = Arc::new(Mutex::new(std::collections::HashMap::<u16, u32>::new()));
        let next_port = Arc::new(AtomicUsize::new(40000));
        let (s2, p2, l2) = (specs.clone(), procs.clone(), layers_by_port.clone());
        let launch: Launch = Box::new(move |spec| {
            s2.lock().unwrap().push((spec.port, spec.gpu_layers, spec.args.clone()));
            l2.lock().unwrap().insert(spec.port, spec.gpu_layers);
            let f = Flags { alive: Arc::new(AtomicBool::new(true)), kills: Arc::new(AtomicUsize::new(0)) };
            p2.lock().unwrap().push(f.clone());
            Ok(Box::new(FakeProc(f)))
        });
        let l3 = layers_by_port.clone();
        let probe: Probe = Box::new(move |port| healthy_layers.is_some() && l3.lock().unwrap().get(&port).copied() == healthy_layers);
        let pick: PickPort = Box::new(move || Ok(next_port.fetch_add(1, Ordering::SeqCst) as u16));
        let mut server = LlmServer::new(launch, probe, pick);
        server.ready_timeout = Duration::from_millis(60);
        server.ready_poll = Duration::from_millis(5);
        Rig { server, specs, procs }
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn args_bind_to_localhost_only_and_use_documented_flags() {
        let a = server_args(Path::new("m.gguf"), 41234, 99);
        let at = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].clone());
        assert_eq!(at("--host").as_deref(), Some("127.0.0.1"));
        assert_eq!(at("--port").as_deref(), Some("41234"));
        assert_eq!(at("-m").as_deref(), Some("m.gguf"));
        assert_eq!(at("-c").as_deref(), Some("8192"));
        assert_eq!(at("-ngl").as_deref(), Some("99"));
        assert!(a.contains(&"--jinja".to_string()));
        assert_eq!(at("--reasoning").as_deref(), Some("off"));
        assert!(!a.iter().any(|x| x == "0.0.0.0" || x == "::"));
        assert_eq!(a.iter().filter(|x| *x == "--host").count(), 1);
        let cpu = server_args(Path::new("m"), 1, 0);
        assert_eq!(cpu[cpu.iter().position(|x| x == "-ngl").unwrap() + 1], "0");
    }

    #[test]
    fn free_port_is_on_localhost_and_bindable() {
        let p = free_port().unwrap();
        assert!(p > 0);
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, p)).is_ok());
    }

    #[test]
    fn starts_once_and_reuses_the_running_server() {
        let r = rig(Some(99));
        let c = no_cancel();
        let p1 = r.server.ensure(Path::new("llama-server"), Path::new("m.gguf"), None, &c).unwrap();
        let p2 = r.server.ensure(Path::new("llama-server"), Path::new("m.gguf"), None, &c).unwrap();
        assert_eq!(p1, p2);
        assert_eq!(r.specs.lock().unwrap().len(), 1);
        assert!(r.server.is_running());
        assert_eq!(r.server.running_port(), Some(p1));
        // 모든 실행이 127.0.0.1 고정
        for (_, _, args) in r.specs.lock().unwrap().iter() {
            let i = args.iter().position(|x| x == "--host").unwrap();
            assert_eq!(args[i + 1], "127.0.0.1");
        }
    }

    #[test]
    fn falls_back_to_cpu_once_when_gpu_start_fails() {
        let r = rig(Some(0));
        let port = r.server.ensure(Path::new("llama-server"), Path::new("m.gguf"), None, &no_cancel()).unwrap();
        let specs = r.specs.lock().unwrap();
        assert_eq!(specs.iter().map(|s| s.1).collect::<Vec<_>>(), vec![99, 0]);
        assert_eq!(specs[1].0, port);
        // 실패한 GPU 서버는 정리됐다.
        let procs = r.procs.lock().unwrap();
        assert_eq!(procs[0].kills.load(Ordering::SeqCst), 1);
        assert!(procs[1].alive.load(Ordering::SeqCst));
    }

    #[test]
    fn reports_failure_after_both_attempts_and_leaves_nothing_running() {
        let r = rig(None);
        let e = r.server.ensure(Path::new("llama-server"), Path::new("m.gguf"), None, &no_cancel()).unwrap_err();
        assert!(matches!(e, LlmError::Unavailable(m) if m.contains("시작하지 못했습니다")));
        assert!(!r.server.is_running());
        assert_eq!(r.specs.lock().unwrap().len(), 2);
        assert!(r.procs.lock().unwrap().iter().all(|p| !p.alive.load(Ordering::SeqCst)));
    }

    #[test]
    fn restarts_when_the_process_died_or_model_changed() {
        let r = rig(Some(99));
        let c = no_cancel();
        r.server.ensure(Path::new("s"), Path::new("a.gguf"), None, &c).unwrap();
        r.procs.lock().unwrap()[0].alive.store(false, Ordering::SeqCst); // 충돌
        r.server.ensure(Path::new("s"), Path::new("a.gguf"), None, &c).unwrap();
        assert_eq!(r.specs.lock().unwrap().len(), 2);
        r.server.ensure(Path::new("s"), Path::new("b.gguf"), None, &c).unwrap();
        assert_eq!(r.specs.lock().unwrap().len(), 3);
        // 이전 서버는 모두 정리되고 마지막 하나만 살아 있다.
        let alive = r.procs.lock().unwrap().iter().filter(|p| p.alive.load(Ordering::SeqCst)).count();
        assert_eq!(alive, 1);
    }

    #[test]
    fn cancel_while_loading_kills_the_starting_process() {
        let r = rig(None);
        let c = AtomicBool::new(true);
        let e = r.server.ensure(Path::new("s"), Path::new("m"), None, &c).unwrap_err();
        assert_eq!(e, LlmError::Cancelled);
        assert!(!r.server.is_running());
        assert!(r.procs.lock().unwrap().iter().all(|p| !p.alive.load(Ordering::SeqCst)));
    }

    #[test]
    fn idle_server_is_stopped_but_not_while_in_use() {
        let mut r = rig(Some(99));
        r.server.idle = Duration::from_secs(300);
        r.server.ensure(Path::new("s"), Path::new("m"), None, &no_cancel()).unwrap();
        let now = Instant::now();
        // 막 썼으면 끄지 않는다.
        assert!(!r.server.sweep(now + Duration::from_secs(299)));
        assert!(r.server.is_running());
        // 사용 중이면 오래 지나도 끄지 않는다.
        let guard = r.server.begin();
        assert!(!r.server.sweep(now + Duration::from_secs(3600)));
        drop(guard); // 놓는 순간 마지막 사용 시각이 갱신된다.
        assert!(!r.server.sweep(Instant::now() + Duration::from_secs(10)));
        // 5분 넘게 쓰지 않으면 끈다.
        assert!(r.server.sweep(Instant::now() + Duration::from_secs(301)));
        assert!(!r.server.is_running());
        assert_eq!(r.procs.lock().unwrap()[0].kills.load(Ordering::SeqCst), 1);
        // 이미 꺼졌으면 아무것도 하지 않는다.
        assert!(!r.server.sweep(Instant::now() + Duration::from_secs(9999)));
    }

    #[test]
    fn default_idle_limit_is_five_minutes() {
        assert_eq!(IDLE_LIMIT, Duration::from_secs(300));
    }

    #[test]
    fn shutdown_and_drop_kill_the_process() {
        let r = rig(Some(99));
        r.server.ensure(Path::new("s"), Path::new("m"), None, &no_cancel()).unwrap();
        r.server.shutdown();
        assert!(!r.server.is_running());
        assert!(!r.procs.lock().unwrap()[0].alive.load(Ordering::SeqCst));
        assert!(r.server.cancel.load(Ordering::SeqCst));
        // 종료 중에는 새로 띄우지 않는다.
        assert!(r.server.ensure(Path::new("s"), Path::new("m"), None, &no_cancel()).is_err());
        assert_eq!(r.specs.lock().unwrap().len(), 1);

        // 놓치지 않는지: 관리자가 사라지면(Drop) 프로세스도 끝난다.
        let r2 = rig(Some(99));
        r2.server.ensure(Path::new("s"), Path::new("m"), None, &no_cancel()).unwrap();
        let procs = r2.procs.clone();
        drop(r2);
        assert!(!procs.lock().unwrap()[0].alive.load(Ordering::SeqCst));
    }

    #[test]
    fn parses_plain_and_chunked_responses() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";
        assert_eq!(parse_response(plain).unwrap(), (200, "{\"status\":\"ok\"}".to_string()));
        let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1,\r\n6\r\n\"b\":2}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked.as_bytes()).unwrap(), (200, "{\"a\":1,\"b\":2}".to_string()));
        let loading = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(loading).unwrap().0, 503);
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn extracts_message_content() {
        let body = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"{\"sentences\":[]}"},"finish_reason":"stop"}]}"#;
        assert_eq!(completion_content(body).unwrap(), "{\"sentences\":[]}");
        assert!(completion_content("{}").is_err());
        assert!(completion_content("no").is_err());
    }

    /// 한 번 응답하고 끝나는 가짜 서버(진짜 소켓)로 http/chat_once를 확인한다.
    fn fake_server(response: &'static str, hold_ms: u64) -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                if let Some(i) = find(&got, b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&got[..i]).to_ascii_lowercase();
                    let want = head.split("content-length:").nth(1).and_then(|r| r.split("\r\n").next()).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                    if got.len() >= i + 4 + want {
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(hold_ms));
            let _ = s.write_all(response.as_bytes());
            String::from_utf8_lossy(&got).into_owned()
        });
        (port, h)
    }

    #[test]
    fn chat_once_posts_json_to_localhost_and_returns_content() {
        let body = r#"{"choices":[{"message":{"content":"안녕"}}]}"#;
        let resp: &'static str = Box::leak(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).into_boxed_str());
        let (port, h) = fake_server(resp, 0);
        let out = chat_once(port, &serde_json::json!({"messages": []}), &no_cancel()).unwrap();
        assert_eq!(out, "안녕");
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(sent.contains("Host: 127.0.0.1:"));
        assert!(sent.ends_with("{\"messages\":[]}"));
    }

    #[test]
    fn http_error_status_and_cancel() {
        let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\n\r\nfail";
        let (port, h) = fake_server(resp, 0);
        let e = chat_once(port, &serde_json::json!({}), &no_cancel()).unwrap_err();
        assert!(matches!(e, LlmError::Failed(m) if m.contains("500") && m.contains("fail")));
        h.join().unwrap();

        // 응답이 늦는 서버: 취소하면 바로 돌아온다.
        let (port, h) = fake_server("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", 1500);
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let e = chat_once(port, &serde_json::json!({}), &cancel).unwrap_err();
        assert_eq!(e, LlmError::Cancelled);
        assert!(started.elapsed() < Duration::from_millis(1200));
        h.join().unwrap();

        // 아무도 듣지 않는 포트
        let closed = free_port().unwrap();
        assert!(matches!(http(closed, "GET", "/health", None, Duration::from_secs(1), &no_cancel()), Err(LlmError::Failed(_))));
    }
}
