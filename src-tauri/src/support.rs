//! 문의용 진단 파일(zip) 만들기.
//!
//! 들어가는 것: `about.json`(버전·사양·개수, 숫자만), `logs/`(오류 기록), `diag/`(기기 점검 결과 최근 3개).
//! 들어가지 않는 것: DB 파일, 사진·음성, 백업 파일, 리포트, 학생 명단.
//! 파일만 만들고 인터넷으로 보내지 않는다. 선생님이 직접 전달한다.
//! zip 형식은 압축 없이(stored) 직접 쓴다. 새 의존성을 늘리지 않기 위해서다.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Datelike, Local, Timelike};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sysinfo::System;
use tauri::{AppHandle, Manager};

use crate::db::{lock, DbResult, DbState, DbStatus};
use crate::diag::{download::is_ready, vulkan, MODELS};
use crate::logging;

/// 기기 점검 결과 파일에서 가져올 최대 개수.
pub const DIAG_KEEP: usize = 3;

// ---------- about.json ----------

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub classes: i64,
    pub students: i64,
    pub observations: i64,
    pub attachments: i64,
    pub units: i64,
    pub habits: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub file_name: String,
    /// 내려받은 파일의 실제 크기(없으면 0)
    pub size_bytes: u64,
    pub exists: bool,
    /// 크기와 확인 표시가 모두 맞는지
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginesInfo {
    pub llama_server: bool,
    pub llama_bench: bool,
    pub whisper_cli: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VulkanSummary {
    pub available: bool,
    pub devices: Vec<String>,
}

/// 앱·PC 정보(화면이 없는 순수 값). 명령이 모아서 채운다.
#[derive(Debug, Clone)]
pub struct Facts {
    pub app_version: String,
    pub curriculum_data_version: String,
    pub os: String,
    pub arch: String,
    pub cpu_brand: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub webview_version: Option<String>,
    pub vulkan: VulkanSummary,
    pub engines: EnginesInfo,
    /// 기록 DB 보호 상태: `ok` | `plain` | `locked`
    pub db_state: &'static str,
    pub db_encrypted: bool,
    /// 사진·음성 파일 중 암호화된 수 / 아직 평문인 수(숫자만)
    pub media_encrypted: u64,
    pub media_plain: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutJson {
    pub created_at: String,
    pub app_version: String,
    pub curriculum_data_version: String,
    pub os: String,
    pub arch: String,
    pub cpu_brand: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub webview_version: Option<String>,
    pub vulkan: VulkanSummary,
    pub engines: EnginesInfo,
    pub models: Vec<ModelInfo>,
    pub db_encrypted: bool,
    pub db_state: &'static str,
    pub media_encrypted: u64,
    pub media_plain: u64,
    pub db_schema_version: i64,
    pub latest_schema_version: i64,
    pub counts: Counts,
}

/// 읽기 전용 개수 조회(숫자만).
pub fn read_counts(conn: &Connection) -> DbResult<Counts> {
    let n = |sql: &str| -> DbResult<i64> { conn.query_row(sql, [], |r| r.get(0)).map_err(|e| format!("개수를 세지 못했습니다: {e}")) };
    Ok(Counts {
        classes: n("SELECT count(*) FROM classes")?,
        students: n("SELECT count(*) FROM students")?,
        observations: n("SELECT count(*) FROM observations WHERE deleted_at IS NULL")?,
        attachments: n("SELECT count(*) FROM attachments")?,
        units: n("SELECT count(*) FROM units")?,
        habits: n("SELECT count(*) FROM habits")?,
    })
}

pub fn read_schema_version(conn: &Connection) -> DbResult<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(|e| format!("DB 버전을 읽지 못했습니다: {e}"))
}

/// 모델 폴더의 파일 상태(크기·확인 여부)만 본다. 파일 내용·경로는 담지 않는다.
pub fn model_infos(models_dir: &Path) -> Vec<ModelInfo> {
    MODELS
        .iter()
        .map(|spec| {
            let path = models_dir.join(spec.file_name);
            let meta = std::fs::metadata(&path).ok();
            ModelInfo {
                id: spec.id.to_string(),
                file_name: spec.file_name.to_string(),
                size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                exists: meta.is_some(),
                verified: is_ready(&path, spec),
            }
        })
        .collect()
}

pub fn build_about(facts: Facts, conn: &Connection, models_dir: &Path, now: DateTime<Local>) -> DbResult<AboutJson> {
    Ok(AboutJson {
        created_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        app_version: facts.app_version,
        curriculum_data_version: facts.curriculum_data_version,
        os: facts.os,
        arch: facts.arch,
        cpu_brand: facts.cpu_brand,
        physical_cores: facts.physical_cores,
        logical_cores: facts.logical_cores,
        total_memory_bytes: facts.total_memory_bytes,
        available_memory_bytes: facts.available_memory_bytes,
        webview_version: facts.webview_version,
        vulkan: facts.vulkan,
        engines: facts.engines,
        models: model_infos(models_dir),
        db_encrypted: facts.db_encrypted,
        db_state: facts.db_state,
        media_encrypted: facts.media_encrypted,
        media_plain: facts.media_plain,
        db_schema_version: read_schema_version(conn)?,
        latest_schema_version: crate::db::migrations::latest_version(),
        counts: read_counts(conn)?,
    })
}

// ---------- 들어갈 파일 모으기 ----------

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub label: String,
    pub bytes: u64,
}

fn label_of(name: &str) -> &'static str {
    if name == "about.json" {
        "앱·PC 정보와 기록 개수(숫자만)"
    } else if name.starts_with("logs/") {
        "오류 기록(학생 이름·메모 없음)"
    } else {
        "기기 점검 결과"
    }
}

pub fn items_of(entries: &[Entry]) -> Vec<Item> {
    entries.iter().map(|e| Item { name: e.name.clone(), label: label_of(&e.name).to_string(), bytes: e.data.len() as u64 }).collect()
}

/// 기기 점검 결과 JSON 중 최근 `DIAG_KEEP`개. 파일 이름이 한글이라 zip 안에서는 영문 이름으로 바꾼다.
fn diag_files(diag_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(diag_dir) else { return Vec::new() };
    let mut found: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let stamp = file.strip_prefix("기기점검-")?.strip_suffix(".json")?.to_string();
            Some((stamp, e.path()))
        })
        .collect();
    // 이름 속 날짜·시각 순서가 곧 시간 순서다.
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.truncate(DIAG_KEEP);
    found.into_iter().map(|(stamp, p)| (format!("diag/device-check-{stamp}.json"), p)).collect()
}

/// zip에 넣을 파일을 모은다. 로그와 기기 점검 결과는 한 번 더 가린다(경로의 사용자 이름, 학생 이름).
pub fn collect_entries(about: &AboutJson, logs_dir: &Path, diag_dir: Option<&Path>, names: &[String]) -> DbResult<Vec<Entry>> {
    let json = serde_json::to_string_pretty(about).map_err(|e| format!("정보 파일을 만들 수 없습니다: {e}"))?;
    let mut entries = vec![Entry { name: "about.json".to_string(), data: json.into_bytes() }];

    for file in logging::existing_log_names(logs_dir) {
        let raw = std::fs::read(logs_dir.join(&file)).map_err(|e| format!("로그 파일을 읽을 수 없습니다({file}): {e}"))?;
        let text = logging::scrub_text(&String::from_utf8_lossy(&raw), names);
        entries.push(Entry { name: format!("logs/{file}"), data: text.into_bytes() });
    }

    if let Some(dir) = diag_dir {
        for (name, path) in diag_files(dir) {
            let Ok(raw) = std::fs::read(&path) else { continue };
            // 점검 결과에는 모델 폴더 경로(사용자 이름 포함)가 들어 있다.
            let text = logging::scrub_text(&String::from_utf8_lossy(&raw), names);
            entries.push(Entry { name, data: text.into_bytes() });
        }
    }
    Ok(entries)
}

// ---------- zip 쓰기(압축 없음) ----------

fn crc32_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

pub fn crc32(data: &[u8]) -> u32 {
    let t = crc32_table();
    !data.iter().fold(!0u32, |c, b| t[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8))
}

fn dos_time_date(now: &DateTime<Local>) -> (u16, u16) {
    let time = ((now.hour() as u16) << 11) | ((now.minute() as u16) << 5) | (now.second() as u16 / 2);
    let year = (now.year().clamp(1980, 2107) - 1980) as u16;
    let date = (year << 9) | ((now.month() as u16) << 5) | now.day() as u16;
    (time, date)
}

/// 압축하지 않는(stored) zip 바이트열.
pub fn zip_bytes(entries: &[Entry], now: &DateTime<Local>) -> DbResult<Vec<u8>> {
    if entries.len() > u16::MAX as usize {
        return Err("파일이 너무 많습니다".to_string());
    }
    let (time, date) = dos_time_date(now);
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for e in entries {
        if e.data.len() > u32::MAX as usize / 2 || out.len() > u32::MAX as usize / 2 {
            return Err("진단 파일이 너무 큽니다".to_string());
        }
        let crc = crc32(&e.data);
        let size = e.data.len() as u32;
        let name = e.name.as_bytes();
        let offset = out.len() as u32;
        // flags 0x0800: 이름이 UTF-8
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&e.data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// 선생님이 고른 경로에 zip을 쓴다(확장자가 없으면 `.zip`).
pub fn write_zip(path: &Path, entries: &[Entry], now: &DateTime<Local>) -> DbResult<PathBuf> {
    let mut out = path.to_path_buf();
    if out.extension().is_none() {
        out.set_extension("zip");
    }
    let bytes = zip_bytes(entries, now)?;
    std::fs::write(&out, bytes).map_err(|e| format!("진단 파일을 저장할 수 없습니다({}): {e}", logging::scrub_paths(&out.to_string_lossy())))?;
    Ok(out)
}

// ---------- 명령 ----------

fn gather_facts(app: &AppHandle, curriculum_data_version: String) -> Facts {
    let db_state = app.try_state::<DbStatus>().map(|s| crate::db::status_of(&s.0)).unwrap_or(DbState::Ok);
    let sys = System::new().ok().map(|mut sys| {
        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys
    });
    let (media_encrypted, media_plain) = app.try_state::<crate::db::MediaDir>().map(|m| crate::db::media::count_state(&m.0)).unwrap_or((0, 0));
    let v = vulkan::probe();
    let has = |dir: &str, name: &str| crate::engine::engine(app, dir, name).is_ok();
    Facts {
        app_version: app.package_info().version.to_string(),
        curriculum_data_version,
        os: System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.to_string()),
        arch: std::env::consts::ARCH.to_string(),
        cpu_brand: sys.as_ref().and_then(|s| s.cpus().first().map(|c| c.brand().trim().to_string())).unwrap_or_default(),
        physical_cores: System::physical_core_count().ok(),
        logical_cores: sys.as_ref().map(|s| s.cpus().len()).unwrap_or(0),
        total_memory_bytes: sys.as_ref().map(|s| s.total_memory()).unwrap_or(0),
        available_memory_bytes: sys.as_ref().map(|s| s.available_memory()).unwrap_or(0),
        webview_version: tauri::webview_version().ok(),
        vulkan: VulkanSummary { available: v.available, devices: v.devices.iter().map(|d| d.name.clone()).collect() },
        engines: EnginesInfo {
            llama_server: has("llama", "llama-server"),
            llama_bench: has("llama", "llama-bench"),
            whisper_cli: has("whisper", "whisper-cli"),
        },
        db_state: db_state.kind(),
        db_encrypted: db_state.encrypted(),
        media_encrypted,
        media_plain,
    }
}

fn gather(app: &AppHandle, curriculum_data_version: String) -> DbResult<Vec<Entry>> {
    let local = crate::paths::local_data_dir(app).map_err(|e| format!("앱 데이터 폴더를 찾을 수 없습니다: {e}"))?;
    let facts = gather_facts(app, curriculum_data_version);
    let db = app.state::<Mutex<Connection>>();
    let (about, names) = {
        let conn = lock(&db)?;
        (build_about(facts, &conn, &local.join("models"), Local::now())?, logging::load_names(&conn))
    };
    let diag_dir = app.path().document_dir().or_else(|_| app.path().desktop_dir()).ok().map(|d| d.join("바슬즐 기기점검"));
    collect_entries(&about, &local.join("logs"), diag_dir.as_deref(), &names)
}

#[tauri::command]
pub async fn support_preview(app: AppHandle, curriculum_data_version: String) -> DbResult<Vec<Item>> {
    tauri::async_runtime::spawn_blocking(move || Ok(items_of(&gather(&app, curriculum_data_version)?)))
        .await
        .map_err(|e| format!("작업이 중단되었습니다: {e}"))?
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInput {
    pub path: String,
    pub curriculum_data_version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResult {
    pub path: String,
    pub items: Vec<Item>,
}

#[tauri::command]
pub async fn support_create(app: AppHandle, input: CreateInput) -> DbResult<CreateResult> {
    let r: DbResult<CreateResult> = tauri::async_runtime::spawn_blocking(move || {
        let entries = gather(&app, input.curriculum_data_version)?;
        let path = write_zip(Path::new(&input.path), &entries, &Local::now())?;
        Ok(CreateResult { path: path.to_string_lossy().into_owned(), items: items_of(&entries) })
    })
    .await
    .map_err(|e| format!("작업이 중단되었습니다: {e}"))
    .and_then(|r| r);
    match &r {
        Ok(res) => logging::info("support", &format!("진단 파일을 만들었습니다({}개 파일)", res.items.len())),
        Err(e) => logging::command_error("support_create", e.as_str()),
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::new_id;
    use chrono::TimeZone;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-support-{}-{}", name, new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn facts() -> Facts {
        Facts {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            curriculum_data_version: "9.9.9".to_string(),
            os: "Windows 11".to_string(),
            arch: "x86_64".to_string(),
            cpu_brand: "Test CPU".to_string(),
            physical_cores: Some(4),
            logical_cores: 8,
            total_memory_bytes: 16 << 30,
            available_memory_bytes: 8 << 30,
            webview_version: Some("1.0".to_string()),
            vulkan: VulkanSummary { available: true, devices: vec!["GPU".to_string()] },
            engines: EnginesInfo { llama_server: true, llama_bench: false, whisper_cli: true },
            db_state: "ok",
            db_encrypted: true,
            media_encrypted: 12,
            media_plain: 3,
        }
    }

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2027, 3, 5, 10, 20, 30).unwrap()
    }

    /// 압축 없는 zip을 읽어 (이름, 내용)을 돌려준다(CRC도 확인).
    fn read_zip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u16at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
        let u32at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        let eocd = bytes.len() - 22;
        assert_eq!(u32at(eocd), 0x0605_4b50);
        let n = u16at(eocd + 10);
        let mut p = u32at(eocd + 16) as usize;
        let mut out = Vec::new();
        for _ in 0..n {
            assert_eq!(u32at(p), 0x0201_4b50);
            let crc = u32at(p + 16);
            let size = u32at(p + 24) as usize;
            let nlen = u16at(p + 28);
            let local = u32at(p + 42) as usize;
            let name = String::from_utf8(bytes[p + 46..p + 46 + nlen].to_vec()).unwrap();
            assert_eq!(u32at(local), 0x0403_4b50);
            let lname = u16at(local + 26);
            let start = local + 30 + lname;
            let data = bytes[start..start + size].to_vec();
            assert_eq!(crc32(&data), crc, "{name} CRC");
            out.push((name, data));
            p += 46 + nlen;
        }
        out
    }

    /// 학급 명단·미디어·백업이 있는 데이터 폴더와 로그·점검 결과가 있는 가짜 PC.
    struct Rig {
        root: PathBuf,
        logs: PathBuf,
        docs: PathBuf,
    }

    fn rig() -> Rig {
        let root = tmp("rig");
        let logs = root.join("logs");
        let docs = root.join("docs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(root.join("data/media")).unwrap();
        std::fs::write(root.join("data/tonghap.db"), b"SQLite format 3 secret").unwrap();
        std::fs::write(root.join("data/media/photo.jpg"), b"jpg").unwrap();
        std::fs::write(root.join("data/media/voice.wav"), b"wav").unwrap();
        std::fs::write(root.join("백업.tonghap"), b"backup").unwrap();
        std::fs::write(root.join("학생명단.txt"), "김하늘\n이도윤").unwrap();
        std::fs::write(logs.join("app.log"), "2027-03-05T10:20:00+09:00 INFO [app] 시작\n").unwrap();
        std::fs::write(logs.join("app.1.log"), "옛 로그 C:\\Users\\김선생\\AppData\\Local\\x 김하늘\n").unwrap();
        for stamp in ["20270301-090000", "20270302-090000", "20270303-090000", "20270304-090000"] {
            std::fs::write(docs.join(format!("기기점검-{stamp}.json")), format!("{{\"modelsDir\":\"C:\\\\Users\\\\김선생\\\\AppData\\\\Local\\\\m\",\"s\":\"{stamp}\"}}")).unwrap();
            std::fs::write(docs.join(format!("기기점검-{stamp}.txt")), "요약").unwrap();
        }
        std::fs::write(docs.join("리포트-김하늘.pdf"), b"pdf").unwrap();
        Rig { root, logs, docs }
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn zip_contains_exactly_the_designed_files() {
        let r = rig();
        let conn = crate::db::test_conn();
        let about = build_about(facts(), &conn, &r.root.join("models"), now()).unwrap();
        let names = vec!["김하늘".to_string(), "이도윤".to_string()];
        let entries = collect_entries(&about, &r.logs, Some(&r.docs), &names).unwrap();
        let bytes = zip_bytes(&entries, &now()).unwrap();
        let files = read_zip(&bytes);
        let listed: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            listed,
            vec![
                "about.json",
                "logs/app.log",
                "logs/app.1.log",
                "diag/device-check-20270304-090000.json",
                "diag/device-check-20270303-090000.json",
                "diag/device-check-20270302-090000.json",
            ]
        );
        // DB·사진·음성·백업·명단·리포트의 이름도 내용도 들어 있지 않다.
        let all: Vec<u8> = bytes.clone();
        let hay = String::from_utf8_lossy(&all);
        for bad in ["SQLite format", "photo.jpg", "voice.wav", "tonghap.db", ".tonghap", "학생명단", "리포트", "김하늘", "이도윤", "김선생"] {
            assert!(!hay.contains(bad), "{bad} 가 들어 있음");
        }
        // 로그와 점검 결과 속 경로는 가려진다.
        let old = files.iter().find(|f| f.0 == "logs/app.1.log").unwrap();
        assert!(String::from_utf8_lossy(&old.1).contains("%LOCALAPPDATA%"));
        let d = files.iter().find(|f| f.0.starts_with("diag/")).unwrap();
        assert!(String::from_utf8_lossy(&d.1).contains("%LOCALAPPDATA%"));
        let _ = std::fs::remove_dir_all(&r.root);
    }

    #[test]
    fn zip_without_logs_or_diag_has_only_about() {
        let root = tmp("empty");
        let conn = crate::db::test_conn();
        let about = build_about(facts(), &conn, &root.join("models"), now()).unwrap();
        let entries = collect_entries(&about, &root.join("logs"), None, &[]).unwrap();
        assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["about.json"]);
        let entries = collect_entries(&about, &root.join("logs"), Some(&root.join("nodocs")), &[]).unwrap();
        assert_eq!(entries.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn about_json_has_expected_fields_and_counts_only() {
        let conn = crate::db::test_conn();
        conn.execute("INSERT INTO classes (id, school_year, grade, name, created_at) VALUES ('c', 2027, 1, '1반', 'x')", []).unwrap();
        conn.execute("INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s1', 'c', 1, '김하늘', 'x')", []).unwrap();
        conn.execute("INSERT INTO students (id, class_id, number, name, created_at) VALUES ('s2', 'c', 2, '이도윤', 'x')", []).unwrap();
        let root = tmp("about");
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::write(root.join("models").join(MODELS[0].file_name), b"partial").unwrap();
        let about = build_about(facts(), &conn, &root.join("models"), now()).unwrap();
        let v = serde_json::to_value(&about).unwrap();
        for key in [
            "createdAt",
            "appVersion",
            "curriculumDataVersion",
            "os",
            "arch",
            "cpuBrand",
            "physicalCores",
            "logicalCores",
            "totalMemoryBytes",
            "availableMemoryBytes",
            "webviewVersion",
            "vulkan",
            "engines",
            "models",
            "dbEncrypted",
            "dbState",
            "mediaEncrypted",
            "mediaPlain",
            "dbSchemaVersion",
            "latestSchemaVersion",
            "counts",
        ] {
            assert!(v.get(key).is_some(), "{key} 없음");
        }
        assert_eq!(v["appVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(v["curriculumDataVersion"], "9.9.9");
        assert_eq!(v["vulkan"]["available"], true);
        assert_eq!(v["engines"]["llamaBench"], false);
        assert_eq!(v["dbEncrypted"], true);
        assert_eq!(v["dbState"], "ok");
        assert_eq!((v["mediaEncrypted"].as_u64(), v["mediaPlain"].as_u64()), (Some(12), Some(3)), "사진·음성은 개수(숫자)만");
        assert_eq!(v["dbSchemaVersion"], v["latestSchemaVersion"]);
        assert_eq!(v["counts"], serde_json::json!({"classes":1,"students":2,"observations":0,"attachments":0,"units":0,"habits":0}));
        let models = v["models"].as_array().unwrap();
        assert_eq!(models.len(), MODELS.len());
        assert_eq!(models[0]["exists"], true);
        assert_eq!(models[0]["sizeBytes"], 7);
        assert_eq!(models[0]["verified"], false);
        assert_eq!(models[1]["exists"], false);
        // 이름이나 경로는 어디에도 없다.
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("김하늘") && !text.contains(root.to_string_lossy().as_ref()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_zip_adds_extension_and_roundtrips() {
        let root = tmp("write");
        let entries = vec![Entry { name: "about.json".into(), data: b"{}".to_vec() }];
        let p = write_zip(&root.join("바슬즐-진단"), &entries, &now()).unwrap();
        assert_eq!(p.extension().unwrap(), "zip");
        let files = read_zip(&std::fs::read(&p).unwrap());
        assert_eq!(files, vec![("about.json".to_string(), b"{}".to_vec())]);
        if let Some(keep) = std::env::var_os("TONGHAP_KEEP_ZIP") {
            std::fs::copy(&p, keep).unwrap();
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn items_have_korean_labels() {
        let items = items_of(&[
            Entry { name: "about.json".into(), data: vec![0; 3] },
            Entry { name: "logs/app.log".into(), data: vec![] },
            Entry { name: "diag/device-check-1.json".into(), data: vec![0; 5] },
        ]);
        assert_eq!(items[0].bytes, 3);
        assert!(items[0].label.contains("앱·PC"));
        assert!(items[1].label.contains("오류 기록"));
        assert!(items[2].label.contains("기기 점검"));
    }
}
