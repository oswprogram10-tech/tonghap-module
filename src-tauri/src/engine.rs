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

pub fn tail(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_last_chars() {
        assert_eq!(tail("가나다라마", 2), "라마");
        assert_eq!(tail("ab", 10), "ab");
    }
}
