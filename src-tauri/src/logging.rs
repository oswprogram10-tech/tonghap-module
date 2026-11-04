//! 오류 기록(로그). `<데이터 폴더의 부모>/logs/app.log`에 한 줄씩 남긴다.
//!
//! 선생님 PC의 문제를 개발자가 알 수 있게 하되, 아이 정보와 개인 정보는 남기지 않는다.
//! - 학생 이름은 `○○`로 가린다(학급 명단을 불러와 대조).
//! - 경로 속 사용자 이름은 `%LOCALAPPDATA%` 같은 표기로 바꾼다.
//! - 명령 인자·메모·자막·백업 암호는 이 모듈에 넘기지 않는다(오류 문구만 받는다).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, FixedOffset, Local, SecondsFormat};
use regex::Regex;
use rusqlite::Connection;
use serde::Deserialize;

/// 로그 한 파일의 최대 크기(넘으면 돌린다).
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// 남기는 로그 파일 수(`app.log`, `app.1.log`, `app.2.log`).
pub const KEEP: usize = 3;
/// 한 줄 메시지의 최대 글자 수.
const MAX_LINE_CHARS: usize = 2000;
/// 화면(프런트엔드) 오류 메시지의 최대 글자 수.
pub const FRONTEND_MAX_CHARS: usize = 300;
/// 학생 이름을 가릴 때 쓰는 표시.
const MASK: &str = "○○";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Error => "ERROR",
        }
    }
}

// ---------- 가리기 ----------

fn path_regexes() -> &'static [(Regex, &'static str)] {
    static RE: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RE.get_or_init(|| {
        // 사용자 이름에는 공백이 들어갈 수 있다("김 선생"). 뒤에 경로 구분자가 오는 경우만 공백을 허용한다.
        let user = r#"[^\\/"'<>|:*?\r\n]+"#;
        let plain = r#"[^\\/"'<>|:*?\s]+"#;
        let sep = r"[\\/]+";
        let build = |p: String, to: &'static str| (Regex::new(&p).unwrap(), to);
        vec![
            build(format!(r"(?i)[A-Z]:{sep}Users{sep}{user}{sep}AppData{sep}Local"), "%LOCALAPPDATA%"),
            build(format!(r"(?i)[A-Z]:{sep}Users{sep}{user}{sep}AppData{sep}Roaming"), "%APPDATA%"),
            build(format!(r"(?i)[A-Z]:{sep}Users{sep}{user}(?:[\\/])"), "%USERPROFILE%\\"),
            build(format!(r"(?i)[A-Z]:{sep}Users{sep}{plain}"), "%USERPROFILE%"),
            build(r#"/(?:home|Users)/[^/\s"']+"#.to_string(), "~"),
        ]
    })
}

/// 경로 속 사용자 이름을 가린다.
pub fn scrub_paths(s: &str) -> String {
    let mut out = s.to_string();
    for (re, to) in path_regexes() {
        out = re.replace_all(&out, *to).into_owned();
    }
    out
}

/// 경로와 학생 이름(두 글자 이상)을 가린다. 줄바꿈은 그대로 둔다.
pub fn scrub_text(s: &str, names: &[String]) -> String {
    let mut out = scrub_paths(s);
    let mut sorted: Vec<&String> = names.iter().filter(|n| n.chars().count() >= 2).collect();
    sorted.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
    for n in sorted {
        out = out.replace(n.as_str(), MASK);
    }
    out
}

/// 글자 수로 자른다(자르면 `…`을 붙임).
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

/// 한 줄 메시지로 만든다: 가리기 → 줄바꿈을 ` ⏎ `로 → 길이 제한.
pub fn sanitize_line(msg: &str, names: &[String]) -> String {
    let scrubbed = scrub_text(msg, names);
    let one = scrubbed.replace("\r\n", " ⏎ ").replace(['\n', '\r'], " ⏎ ");
    truncate_chars(one.trim(), MAX_LINE_CHARS)
}

/// `2027-03-05T10:20:00+09:00 ERROR [backup] 메시지`
pub fn format_line(now: &DateTime<FixedOffset>, level: Level, tag: &str, msg: &str) -> String {
    format!("{} {} [{}] {}\n", now.to_rfc3339_opts(SecondsFormat::Secs, false), level.as_str(), tag, msg)
}

// ---------- 파일 ----------

pub fn log_file_name(index: usize) -> String {
    if index == 0 {
        "app.log".to_string()
    } else {
        format!("app.{index}.log")
    }
}

/// 로그 폴더에 있는 로그 파일 이름(최신 먼저).
pub fn existing_log_names(dir: &Path) -> Vec<String> {
    (0..KEEP).map(log_file_name).filter(|n| dir.join(n).is_file()).collect()
}

pub struct Logger {
    dir: PathBuf,
    max_bytes: u64,
    names: Vec<String>,
}

impl Logger {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, max_bytes: MAX_BYTES, names: Vec::new() }
    }

    #[cfg(test)]
    fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn set_names(&mut self, names: Vec<String>) {
        self.names = names;
    }

    /// `app.log`가 한도를 넘으면 `app.1.log`, `app.2.log`로 밀고 가장 오래된 것을 지운다.
    fn rotate_if_needed(&self) {
        let current = self.dir.join(log_file_name(0));
        let size = fs::metadata(&current).map(|m| m.len()).unwrap_or(0);
        if size < self.max_bytes {
            return;
        }
        let _ = fs::remove_file(self.dir.join(log_file_name(KEEP - 1)));
        for i in (0..KEEP - 1).rev() {
            let from = self.dir.join(log_file_name(i));
            if from.exists() {
                let _ = fs::rename(&from, self.dir.join(log_file_name(i + 1)));
            }
        }
    }

    pub fn write(&self, now: &DateTime<FixedOffset>, level: Level, tag: &str, msg: &str) {
        let line = format_line(now, level, tag, &sanitize_line(msg, &self.names));
        let _ = fs::create_dir_all(&self.dir);
        self.rotate_if_needed();
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(self.dir.join(log_file_name(0))) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

// ---------- 앱 전체에서 쓰는 로거 ----------

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

/// 앱 시작 때 한 번 부른다. 폴더는 `<데이터 폴더의 부모>/logs`.
pub fn init(dir: PathBuf) {
    *LOGGER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Logger::new(dir));
}

/// 로그를 끈다(테스트용).
#[cfg(test)]
fn reset() {
    *LOGGER.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn log(level: Level, tag: &str, msg: &str) {
    if let Some(l) = LOGGER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        l.write(&now(), level, tag, msg);
    }
}

pub fn info(tag: &str, msg: &str) {
    log(Level::Info, tag, msg);
}

pub fn error(tag: &str, msg: &str) {
    log(Level::Error, tag, msg);
}

/// 명령 이름과 오류 문구만 남긴다(인자는 받지 않는다).
pub fn command_error(command: &str, err: &str) {
    error("command", &format!("{command}: {err}"));
}

/// 결과가 오류면 기록하고 그대로 돌려준다.
pub fn log_err<T>(tag: &str, what: &str, r: Result<T, String>) -> Result<T, String> {
    if let Err(e) = &r {
        error(tag, &format!("{what} 실패: {e}"));
    }
    r
}

/// 학생 이름 목록(가리기용)을 새로 불러온다.
pub fn refresh_names(conn: &Connection) {
    let names = load_names(conn);
    if let Some(l) = LOGGER.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        l.set_names(names);
    }
}

pub fn load_names(conn: &Connection) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT DISTINCT name FROM students") else { return Vec::new() };
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    match rows {
        Ok(it) => it.flatten().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect(),
        Err(_) => Vec::new(),
    }
}

// ---------- 패닉 훅 ----------

fn panic_text(payload: &(dyn std::any::Any + Send), location: Option<&std::panic::Location<'_>>) -> String {
    let msg = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(메시지 없음)".to_string());
    match location {
        Some(l) => format!("{}:{} {msg}", l.file(), l.line()),
        None => msg,
    }
}

/// 패닉이 나면 위치와 메시지를 기록한 뒤 원래 훅(기본 출력)을 이어서 부른다.
/// 로거를 쓰는 중에 패닉이 난 경우 멈추지 않도록 잠금은 기다리지 않는다.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = panic_text(info.payload(), info.location());
        if let Ok(guard) = LOGGER.try_lock() {
            if let Some(l) = guard.as_ref() {
                l.write(&now(), Level::Error, "panic", &text);
            }
        }
        previous(info);
    }));
}

// ---------- 화면(프런트엔드) 오류 ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendError {
    pub message: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub line: Option<u32>,
}

/// `[frontend] 메시지 (출처:줄)` 형태의 본문. 메시지는 300자로 자른다.
pub fn frontend_message(e: &FrontendError) -> String {
    let msg = truncate_chars(&e.message, FRONTEND_MAX_CHARS);
    let src = truncate_chars(&e.source, FRONTEND_MAX_CHARS);
    match (src.is_empty(), e.line) {
        (true, _) => msg,
        (false, Some(l)) => format!("{msg} ({src}:{l})"),
        (false, None) => format!("{msg} ({src})"),
    }
}

#[tauri::command]
pub fn log_frontend_error(db: tauri::State<Mutex<Connection>>, input: FrontendError) {
    // 이름 목록은 잠깐만 기다려 보고, 바쁘면 이전 목록으로 가린다.
    if let Ok(conn) = db.try_lock() {
        refresh_names(&conn);
    }
    error("frontend", &frontend_message(&input));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::new_id;
    use chrono::TimeZone;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-log-{}-{}", name, new_id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn at() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(9 * 3600).unwrap().with_ymd_and_hms(2027, 3, 5, 10, 20, 0).unwrap()
    }

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap_or_default()
    }

    #[test]
    fn line_format_matches_design() {
        assert_eq!(
            format_line(&at(), Level::Error, "backup", "메시지"),
            "2027-03-05T10:20:00+09:00 ERROR [backup] 메시지\n"
        );
        assert!(format_line(&at(), Level::Info, "app", "x").contains(" INFO [app] x"));
    }

    #[test]
    fn writes_one_line_per_call_and_folds_newlines() {
        let dir = tmp("lines");
        let l = Logger::new(dir.clone());
        l.write(&at(), Level::Info, "app", "첫째");
        l.write(&at(), Level::Error, "engine", "종료 코드 1\n둘째 줄\r\n셋째 줄");
        let text = read(&dir, "app.log");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("종료 코드 1 ⏎ 둘째 줄 ⏎ 셋째 줄"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotates_past_limit_and_keeps_three_files() {
        let dir = tmp("rotate");
        let l = Logger::new(dir.clone()).with_max_bytes(200);
        for i in 0..40 {
            l.write(&at(), Level::Info, "app", &format!("줄 번호 {i:02} {}", "가".repeat(20)));
        }
        let names = existing_log_names(&dir);
        assert_eq!(names, vec!["app.log", "app.1.log", "app.2.log"]);
        assert!(!dir.join("app.3.log").exists());
        // 가장 새 줄은 app.log에, 오래된 줄은 밀려나 사라진다.
        assert!(read(&dir, "app.log").contains("줄 번호 39"));
        assert!(!names.iter().any(|n| read(&dir, n).contains("줄 번호 00 ")));
        // 한 파일은 한도를 크게 넘지 않는다(한 줄 길이 범위).
        for n in &names {
            assert!(fs::metadata(dir.join(n)).unwrap().len() < 200 + 160);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_rotate_below_limit() {
        let dir = tmp("norotate");
        let l = Logger::new(dir.clone());
        for _ in 0..5 {
            l.write(&at(), Level::Info, "app", "짧음");
        }
        assert_eq!(existing_log_names(&dir), vec!["app.log"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scrubs_user_names_in_paths() {
        assert_eq!(
            scrub_paths(r"열 수 없음 C:\Users\김선생\AppData\Local\io.github.x\data\tonghap.db 입니다"),
            r"열 수 없음 %LOCALAPPDATA%\io.github.x\data\tonghap.db 입니다"
        );
        assert_eq!(scrub_paths(r"c:\users\Kim Sun\AppData\Roaming\a"), r"%APPDATA%\a");
        assert_eq!(scrub_paths(r"D:\Users\Kim Sun\Documents\바슬즐 기기점검\a.json"), r"%USERPROFILE%\Documents\바슬즐 기기점검\a.json");
        assert_eq!(scrub_paths("C:/Users/lee/AppData/Local/x/y"), "%LOCALAPPDATA%/x/y");
        assert_eq!(scrub_paths(r"C:\Users\lee"), "%USERPROFILE%");
        assert_eq!(scrub_paths("/home/teacher/x/y 와 /Users/park/z"), "~/x/y 와 ~/z");
        for s in ["김선생", "Kim Sun", "teacher", "park"] {
            let out = scrub_paths(&format!(r"C:\Users\{s}\AppData\Local\a /home/{s}/b"));
            assert!(!out.contains(s), "{out}");
        }
        // 사용자 폴더가 아닌 경로는 그대로 둔다.
        assert_eq!(scrub_paths(r"C:\Program Files\x"), r"C:\Program Files\x");
    }

    #[test]
    fn class_roster_names_never_reach_the_file() {
        let conn = crate::db::test_conn();
        conn.execute("INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 'x')", []).unwrap();
        for (i, n) in ["김하늘", "이도윤", "박서연"].iter().enumerate() {
            conn.execute(
                "INSERT INTO students (id, class_id, number, name, created_at) VALUES (?1, 'c', ?2, ?3, 'x')",
                rusqlite::params![format!("s{i}"), i as i64 + 1, n],
            )
            .unwrap();
        }
        let names = load_names(&conn);
        assert_eq!(names.len(), 3);

        let dir = tmp("names");
        let mut l = Logger::new(dir.clone());
        l.set_names(names);
        l.write(&at(), Level::Error, "command", "observation_create: 김하늘 학생의 기록을 저장할 수 없습니다(이도윤, 박서연과 같은 번호)");
        l.write(&at(), Level::Error, "frontend", "TypeError: 박서연 is undefined (C:\\Users\\김하늘\\AppData\\Local\\app)");
        let text = read(&dir, "app.log");
        for n in ["김하늘", "이도윤", "박서연"] {
            assert!(!text.contains(n), "{n} 가 로그에 남음: {text}");
        }
        assert!(text.contains("○○ 학생의 기록을 저장할 수 없습니다"));
        // 내보낼 때 쓰는 본문 가리기도 같은 규칙이다.
        assert!(!scrub_text("김하늘 메모", &load_names(&conn)).contains("김하늘"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_char_names_are_not_masked_but_long_lines_are_cut() {
        assert_eq!(scrub_text("가나다", &["가".to_string()]), "가나다");
        let s = sanitize_line(&"a".repeat(5000), &[]);
        assert_eq!(s.chars().count(), MAX_LINE_CHARS + 1);
    }

    #[test]
    fn frontend_message_is_cut_to_300_chars() {
        let e = FrontendError { message: "가".repeat(1000), source: "app.js".into(), line: Some(12) };
        let m = frontend_message(&e);
        assert!(m.starts_with(&"가".repeat(300)));
        assert!(!m.contains(&"가".repeat(301)));
        assert!(m.ends_with("(app.js:12)"));
        let e = FrontendError { message: "x".into(), source: String::new(), line: None };
        assert_eq!(frontend_message(&e), "x");
        let e: FrontendError = serde_json::from_str(r#"{"message":"m"}"#).unwrap();
        assert_eq!(frontend_message(&e), "m");
    }

    #[test]
    fn panic_hook_writes_location_and_message_to_the_log() {
        let dir = tmp("panic");
        init(dir.clone());
        install_panic_hook();
        let r = std::panic::catch_unwind(|| {
            panic!("테스트 패닉 C:\\Users\\김선생\\AppData\\Local\\x");
        });
        assert!(r.is_err());
        // 훅을 되돌리고 로거를 끈 뒤 읽는다.
        let _ = std::panic::take_hook();
        reset();
        let text = read(&dir, "app.log");
        assert!(text.contains("ERROR [panic]"), "{text}");
        assert!(text.contains("테스트 패닉 %LOCALAPPDATA%"), "{text}");
        assert!(text.contains("logging.rs:"), "{text}");
        assert!(!text.contains("김선생"));
        let _ = fs::remove_dir_all(&dir);
    }
}
