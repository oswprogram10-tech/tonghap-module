//! 번들된 추론 엔진(llama.cpp, whisper.cpp) 실행 도우미. 기기 점검과 음성 자막이 함께 쓴다.
//! 엔진 파일은 scripts/fetch-engines.mjs가 src-tauri/resources/engines/에 넣는다.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

pub fn resources(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .resource_dir()
        .map_err(|e| format!("리소스 폴더를 찾을 수 없음: {e}"))?
        .join("resources"))
}

/// `resources/engines/<dir>/<name>(.exe)` 경로. 파일이 없으면 안내 문구를 돌려준다.
pub fn engine_in(resources: &Path, dir: &str, name: &str) -> Result<PathBuf, String> {
    let path = resources.join("engines").join(dir).join(exe(name));
    if !path.exists() {
        return Err(format!(
            "엔진 파일이 없습니다({}). 개발 환경이라면 'npm run fetch:engines'를 실행하세요",
            path.display()
        ));
    }
    Ok(path)
}

pub fn engine(app: &AppHandle, dir: &str, name: &str) -> Result<PathBuf, String> {
    engine_in(&resources(app)?, dir, name)
}

pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
}

/// 콘솔 창 없이 뜨는 명령을 만든다(윈도우의 CREATE_NO_WINDOW). 작업 폴더는 실행 파일 옆.
pub fn hidden_command(program: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        // DLL·백엔드 모듈을 실행 파일 옆에서 찾도록 작업 폴더를 맞춘다.
        .current_dir(program.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 콘솔 창 없이 실행하고, 시간이 지나면 강제 종료한다.
pub fn run(program: &Path, args: &[String], timeout: Duration) -> Result<RunOutput, String> {
    let mut cmd = hidden_command(program, args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("실행 실패({}): {e}", program.display()))?;
    attach_to_app_job(&child);

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
        let code = status.code().map_or_else(|| "알 수 없음".to_string(), |c| c.to_string());
        return Err(format!("종료 코드 {code}\n{}", tail(&stderr, 1500)));
    }
    Ok(RunOutput { stdout, stderr, elapsed })
}

/// 윈도우 작업 개체(Job Object) 핸들. 닫으면 속한 프로세스가 모두 종료된다.
#[cfg(windows)]
pub struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

// 핸들은 프로세스 전체에서 유효한 커널 객체 값이라 스레드 사이로 옮겨도 안전하다.
#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
impl JobHandle {
    /// 핸들을 닫는다(KILL_ON_JOB_CLOSE이므로 속한 프로세스가 종료된다).
    pub fn close(self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`가 걸린 새 작업 개체를 만든다.
#[cfg(windows)]
pub fn create_kill_on_close_job() -> Option<JobHandle> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::System::JobObjects::*;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const std::ffi::c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            windows_sys::Win32::Foundation::CloseHandle(job);
            return None;
        }
        Some(JobHandle(job))
    }
}

/// 자식 프로세스를 주어진 작업 개체에 넣는다. 실패하면 false.
#[cfg(windows)]
pub fn assign_to_job(job: &JobHandle, child: &std::process::Child) -> bool {
    use std::os::windows::io::AsRawHandle;
    unsafe {
        windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(job.0, child.as_raw_handle() as _) != 0
    }
}

/// 앱이 비정상 종료(작업 관리자로 끝내기, 충돌)되어도 자식 프로세스가 남지 않게 한다.
/// 윈도우에서 앱 전체가 공유하는 작업 개체(KILL_ON_JOB_CLOSE)에 자식을 넣어 두면,
/// 앱 프로세스가 사라질 때 OS가 핸들을 닫으면서 자식(llama-server 등, 메모리 약 2.5GB)도 함께 종료한다.
/// 실패해도 자식은 그대로 실행되고, 다른 플랫폼에서는 아무 일도 하지 않는다.
pub fn attach_to_app_job(child: &std::process::Child) {
    #[cfg(windows)]
    {
        static APP_JOB: std::sync::OnceLock<Option<JobHandle>> = std::sync::OnceLock::new();
        if let Some(job) = APP_JOB.get_or_init(create_kill_on_close_job) {
            let _ = assign_to_job(job, child);
        }
    }
    #[cfg(not(windows))]
    let _ = child;
}

pub fn tail(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn attach_to_app_job_is_noop_off_windows() {
        let mut child = Command::new("true").spawn().unwrap();
        attach_to_app_job(&child);
        assert!(child.wait().unwrap().success());
    }

    #[test]
    #[cfg(windows)]
    fn closing_job_kills_attached_child() {
        let mut child = Command::new("cmd").args(["/c", "ping", "-n", "30", "127.0.0.1"]).stdout(Stdio::null()).spawn().unwrap();
        let job = create_kill_on_close_job().expect("작업 개체 생성");
        assert!(assign_to_job(&job, &child), "작업 개체에 넣기");
        assert!(child.try_wait().unwrap().is_none());
        job.close();
        let start = Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "자식이 종료되지 않음");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn tail_keeps_last_chars() {
        assert_eq!(tail("가나다라마", 2), "라마");
        assert_eq!(tail("ab", 10), "ab");
    }
}
