//! 음성 메모 자동 자막: 화면이 보낸 16kHz 모노 WAV를 대기열에 넣고, 백그라운드 스레드가
//! whisper-cli로 한 번에 하나씩 받아쓴다. 결과는 `attachments.transcript`에 저장한다.
//! 설계: docs/design/05-voice-transcription.md

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{attachments, lock, now_iso, settings, DbExt, DbResult, MediaDir};
use crate::diag::download::is_ready;
use crate::diag::{model_path, model_spec};
use crate::engine::{engine, run, tail};

pub const MODEL_DEFAULT: &str = "whisper-small-q5";
pub const MODEL_LIGHT: &str = "whisper-base-q5";
pub const MAX_SECONDS: f64 = 300.0;
pub const STT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const EVENT: &str = "stt://updated";

type Engine<'a> = &'a dyn Fn(&Path, &Path) -> Result<String, String>;

/// 앱 시작 때 만들어 관리 상태로 두는 대기열 송신 쪽.
pub struct SttQueue(Mutex<Sender<String>>);

/// 임시 WAV 폴더(`data/stt-tmp`). 미디어 폴더(`data/media`)와 같은 부모 아래에 둔다.
pub fn tmp_dir(media: &Path) -> PathBuf {
    media.parent().unwrap_or(media).join("stt-tmp")
}

fn tmp_wav(tmp: &Path, id: &str) -> PathBuf {
    tmp.join(format!("{id}.wav"))
}

// ---------- WAV 검사 ----------

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// RIFF/WAVE, PCM16, 모노, 16000Hz인지 확인하고 길이(초)를 돌려준다.
pub fn validate_wav(data: &[u8]) -> Result<f64, String> {
    const BAD: &str = "자막용 음성 형식이 올바르지 않습니다(16kHz 모노 WAV)";
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(BAD.to_string());
    }
    let mut pos = 12usize;
    let mut fmt_ok = false;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = le32(&data[pos + 4..pos + 8]) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            if size < 16 || body + 16 > data.len() {
                return Err(BAD.to_string());
            }
            let f = &data[body..body + 16];
            if le16(f) != 1 {
                return Err("자막용 음성은 PCM 형식이어야 합니다".to_string());
            }
            if le16(&f[2..]) != 1 {
                return Err("자막용 음성은 모노여야 합니다".to_string());
            }
            if le32(&f[4..]) != 16000 {
                return Err("자막용 음성은 16kHz여야 합니다".to_string());
            }
            if le16(&f[14..]) != 16 {
                return Err("자막용 음성은 16비트여야 합니다".to_string());
            }
            fmt_ok = true;
        } else if id == b"data" {
            if !fmt_ok {
                return Err(BAD.to_string());
            }
            let len = size.min(data.len() - body);
            let seconds = len as f64 / 32000.0;
            if len == 0 {
                return Err("녹음된 소리가 없습니다".to_string());
            }
            if seconds > MAX_SECONDS {
                return Err("녹음이 너무 깁니다(5분 이하)".to_string());
            }
            return Ok(seconds);
        }
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(BAD.to_string())
}

// ---------- 결과 정리 ----------

fn bracket_only() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s*(\[[^\]]*\]|\([^)]*\)))+\s*$").unwrap())
}

/// whisper-cli 출력에서 `[BLANK_AUDIO]`·`(웃음)` 같은 표시만 있는 줄을 빼고 공백을 정리한다.
pub fn clean_transcript(raw: &str) -> Result<String, String> {
    let lines: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !bracket_only().is_match(l))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if lines.is_empty() {
        return Err("말소리를 찾지 못했습니다".to_string());
    }
    Ok(lines.join("\n"))
}

// ---------- 엔진 인자 ----------

/// 받아쓰기에 쓸 스레드 수(물리 코어 수, 1~8).
pub fn whisper_threads() -> usize {
    sysinfo::System::physical_core_count()
        .or_else(|_| std::thread::available_parallelism().map(|n| n.get()))
        .unwrap_or(4)
        .clamp(1, 8)
}

/// whisper-cli 인자. 앱의 자막은 `lang`을 `ko`로 부른다(엔진 통합 테스트도 같은 함수를 쓴다).
pub fn whisper_args(model: &Path, wav: &Path, lang: &str) -> Vec<String> {
    vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-l".into(),
        lang.into(),
        "-t".into(),
        whisper_threads().to_string(),
        "-nt".into(),
        "-np".into(),
        // --prompt(한국어 안내문)는 넣지 않는다. whisper-cli는 윈도우에서 인자를 시스템 코드 페이지
        // (한국어 윈도우는 CP949)로 받는데 프롬프트는 UTF-8로 토큰화하므로 글자가 깨진다.
    ]
}

// ---------- 설정 ----------

pub fn model_id_setting(conn: &Connection) -> String {
    match settings::get(conn, "sttModelId") {
        Ok(serde_json::Value::String(s)) if s == MODEL_DEFAULT || s == MODEL_LIGHT => s,
        _ => MODEL_DEFAULT.to_string(),
    }
}

pub fn auto_setting(conn: &Connection) -> bool {
    !matches!(settings::get(conn, "sttAuto"), Ok(serde_json::Value::Bool(false)))
}

// ---------- 대기열 상태 (DB) ----------

/// 검사를 통과한 WAV를 임시 폴더에 쓰고 첨부를 `queued`로 바꾼다.
pub fn prepare_job(conn: &Connection, media: &Path, attachment_id: &str, wav: &[u8]) -> DbResult<()> {
    let a = attachments::get(conn, media, attachment_id)?;
    if a.kind != "audio" {
        return Err("음성 메모만 자막을 만들 수 있습니다".to_string());
    }
    if a.transcript_status == "queued" || a.transcript_status == "running" {
        return Err("이미 자막을 만드는 중입니다".to_string());
    }
    validate_wav(wav)?;
    let tmp = tmp_dir(media);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("임시 폴더를 만들 수 없습니다: {e}"))?;
    std::fs::write(tmp_wav(&tmp, attachment_id), wav).map_err(|e| format!("임시 음성 파일을 쓰지 못했습니다: {e}"))?;
    conn.execute(
        "UPDATE attachments SET transcript_status = 'queued', transcript_error = NULL WHERE id = ?1",
        [attachment_id],
    )
    .db()?;
    Ok(())
}

/// 앱 시작·백업 복원 뒤: 임시 WAV가 없으므로 대기·진행 중이던 상태를 되돌린다.
pub fn reset_pending(conn: &Connection) -> DbResult<usize> {
    conn.execute(
        "UPDATE attachments SET transcript_status = 'none' WHERE transcript_status IN ('queued', 'running')",
        [],
    )
    .db()
}

pub fn queue_length(conn: &Connection) -> DbResult<i64> {
    conn.query_row(
        "SELECT count(*) FROM attachments WHERE transcript_status IN ('queued', 'running')",
        [],
        |r| r.get(0),
    )
    .db()
}

fn status_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row("SELECT transcript_status FROM attachments WHERE id = ?1", [id], |r| r.get(0)).ok()
}

/// 작업 하나를 처리한다. DB 잠금은 읽고 쓸 때만 잡고, 엔진이 도는 동안에는 풀어 둔다.
pub fn process_job(
    db: &Mutex<Connection>,
    tmp: &Path,
    id: &str,
    model_id: &str,
    model: &Path,
    run_engine: Engine<'_>,
    notify: &dyn Fn(&str, &str),
) {
    let wav = tmp_wav(tmp, id);
    let started = (|| -> DbResult<bool> {
        let conn = lock(db)?;
        if status_of(&conn, id).as_deref() != Some("queued") {
            return Ok(false);
        }
        conn.execute("UPDATE attachments SET transcript_status = 'running' WHERE id = ?1", [id]).db()?;
        Ok(true)
    })();
    if !matches!(started, Ok(true)) {
        let _ = std::fs::remove_file(&wav);
        return;
    }
    notify(id, "running");

    let result = run_engine(model, &wav).and_then(|raw| clean_transcript(&raw));
    let _ = std::fs::remove_file(&wav);

    let saved = (|| -> DbResult<(usize, &'static str)> {
        let conn = lock(db)?;
        Ok(match &result {
            Ok(text) => (
                conn.execute(
                    "UPDATE attachments SET transcript = ?2, transcript_status = 'done', transcript_error = NULL,
                       transcript_model = ?3, transcribed_at = ?4, transcript_edited = 0
                     WHERE id = ?1 AND transcript_status = 'running'",
                    params![id, text, model_id, now_iso()],
                )
                .db()?,
                "done",
            ),
            Err(e) => {
                let msg = if e == "말소리를 찾지 못했습니다" {
                    e.clone()
                } else {
                    format!("자막을 만들지 못했습니다.\n{}", tail(e.trim(), 400))
                };
                (
                    conn.execute(
                        "UPDATE attachments SET transcript_status = 'failed', transcript_error = ?2
                         WHERE id = ?1 AND transcript_status = 'running'",
                        params![id, msg],
                    )
                    .db()?,
                    "failed",
                )
            }
        })
    })();
    if let Ok((n, status)) = saved {
        if n > 0 {
            notify(id, status);
        }
    }
}

/// 채널이 닫힐 때까지 대기열을 순서대로 처리한다.
pub fn run_queue(
    rx: Receiver<String>,
    db: &Mutex<Connection>,
    tmp: &Path,
    model: &dyn Fn() -> (String, PathBuf),
    run_engine: Engine<'_>,
    notify: &dyn Fn(&str, &str),
) {
    for id in rx {
        let (model_id, path) = model();
        process_job(db, tmp, &id, &model_id, &path, run_engine, notify);
    }
}

/// 앱 시작 때 호출한다(DB·미디어 폴더가 관리 상태로 등록된 뒤).
pub fn start(app: &AppHandle) -> Result<(), String> {
    let media = app.state::<MediaDir>().0.clone();
    let tmp = tmp_dir(&media);
    {
        let db = app.state::<Mutex<Connection>>();
        reset_pending(&*lock(&db)?)?;
    }
    // 남은 임시 WAV 정리
    let _ = std::fs::remove_dir_all(&tmp);

    let (tx, rx) = channel::<String>();
    app.manage(SttQueue(Mutex::new(tx)));

    let app = app.clone();
    std::thread::Builder::new()
        .name("stt-worker".into())
        .spawn(move || {
            let db = app.state::<Mutex<Connection>>();
            let model = || {
                let id = lock(&db).map(|c| model_id_setting(&c)).unwrap_or_else(|_| MODEL_DEFAULT.to_string());
                // 화면 E2E(디버그 빌드): 테스트용 작은 모델 파일을 쓴다.
                let path = crate::webdriver::stt_model_override()
                    .or_else(|| model_spec(&id).and_then(|s| model_path(&app, s)).ok())
                    .unwrap_or_default();
                (id, path)
            };
            let engine_fn = |model: &Path, wav: &Path| -> Result<String, String> {
                if crate::webdriver::stt_model_override().as_deref() == Some(model) {
                    let program = engine(&app, "whisper", "whisper-cli")?;
                    return run(&program, &whisper_args(model, wav, "ko"), STT_TIMEOUT).map(|o| o.stdout);
                }
                let name = model.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let spec = crate::diag::MODELS
                    .iter()
                    .find(|m| m.file_name == name)
                    .ok_or_else(|| "음성 자막 모델이 없습니다".to_string())?;
                if !is_ready(model, spec) {
                    return Err("음성 자막 모델이 없습니다. 기기 점검 탭에서 내려받아 주세요".to_string());
                }
                let program = engine(&app, "whisper", "whisper-cli")?;
                let args = whisper_args(model, wav, "ko");
                run(&program, &args, STT_TIMEOUT).map(|o| o.stdout)
            };
            let notify = |id: &str, status: &str| {
                let _ = app.emit(EVENT, serde_json::json!({ "attachmentId": id, "status": status }));
            };
            run_queue(rx, &db, &tmp, &model, &engine_fn, &notify);
        })
        .map_err(|e| format!("자막 작업자를 시작하지 못했습니다: {e}"))?;
    Ok(())
}

// ---------- Tauri 명령 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SttStatus {
    pub model_id: String,
    pub model_ready: bool,
    pub engine_ready: bool,
    pub auto: bool,
    pub queue_length: i64,
}

#[tauri::command]
pub fn stt_status(app: AppHandle, db: State<Mutex<Connection>>) -> DbResult<SttStatus> {
    let (model_id, auto, queue_length) = {
        let conn = lock(&db)?;
        (model_id_setting(&conn), auto_setting(&conn), queue_length(&conn)?)
    };
    let spec = model_spec(&model_id)?;
    Ok(SttStatus {
        model_ready: crate::webdriver::stt_model_override().is_some_and(|p| p.exists()) || is_ready(&model_path(&app, spec)?, spec),
        engine_ready: engine(&app, "whisper", "whisper-cli").is_ok(),
        model_id,
        auto,
        queue_length,
    })
}

#[tauri::command]
pub fn stt_set_model(db: State<Mutex<Connection>>, model_id: String) -> DbResult<()> {
    if model_id != MODEL_DEFAULT && model_id != MODEL_LIGHT {
        return Err("자막에 쓸 수 없는 모델입니다".to_string());
    }
    settings::set(&*lock(&db)?, "sttModelId", &serde_json::json!(model_id))?;
    Ok(())
}

#[tauri::command]
pub fn stt_set_auto(db: State<Mutex<Connection>>, auto: bool) -> DbResult<()> {
    settings::set(&*lock(&db)?, "sttAuto", &serde_json::json!(auto))?;
    Ok(())
}

#[derive(Serialize)]
pub struct Queued {
    pub status: &'static str,
}

/// 원시 WAV 본문 + 헤더 `x-attachment-id`.
#[tauri::command]
pub fn stt_transcribe(
    request: Request<'_>,
    db: State<Mutex<Connection>>,
    media: State<MediaDir>,
    queue: State<SttQueue>,
) -> DbResult<Queued> {
    let InvokeBody::Raw(data) = request.body() else {
        return Err("원시 바이트 본문이 필요합니다".to_string());
    };
    let id = request
        .headers()
        .get("x-attachment-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or("요청 헤더 x-attachment-id이(가) 없습니다")?;
    {
        let conn = lock(&db)?;
        prepare_job(&conn, &media.0, &id, data)?;
    }
    let sent = queue.0.lock().map_err(|_| "자막 대기열이 잠겨 있습니다".to_string())?.send(id.clone());
    if sent.is_err() {
        let conn = lock(&db)?;
        let _ = conn.execute("UPDATE attachments SET transcript_status = 'none' WHERE id = ?1", [&id]);
        return Err("자막 작업자가 멈춰 있습니다. 앱을 다시 시작해 주세요".to_string());
    }
    Ok(Queued { status: "queued" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::new_id;
    use crate::db::observations::tests::seed;
    use std::cell::RefCell;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-stt-{}-{}", name, new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn wav(rate: u32, channels: u16, bits: u16, data_len: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        v.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        v.extend_from_slice(&bits.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data_len as u32).to_le_bytes());
        v.extend(std::iter::repeat_n(0u8, data_len));
        v
    }

    #[test]
    fn wav_header_validation() {
        let secs = validate_wav(&wav(16000, 1, 16, 32000 * 2)).unwrap();
        assert!((secs - 2.0).abs() < 1e-9);
        assert!(validate_wav(&wav(44100, 1, 16, 1000)).unwrap_err().contains("16kHz"));
        assert!(validate_wav(&wav(16000, 2, 16, 1000)).unwrap_err().contains("모노"));
        assert!(validate_wav(&wav(16000, 1, 8, 1000)).unwrap_err().contains("16비트"));
        assert!(validate_wav(b"not a wav file at all").is_err());
        let mut riff = wav(16000, 1, 16, 1000);
        riff[0..4].copy_from_slice(b"RIFX");
        assert!(validate_wav(&riff).is_err());
        assert!(validate_wav(&wav(16000, 1, 16, 32000 * 300)).is_ok());
        assert!(validate_wav(&wav(16000, 1, 16, 32000 * 300 + 2)).unwrap_err().contains("너무 깁니다"));
        assert!(validate_wav(&wav(16000, 1, 16, 0)).is_err());
    }

    #[test]
    fn clean_transcript_rules() {
        assert_eq!(clean_transcript("  안녕   하세요 \n\n [BLANK_AUDIO]\n(웃음)\n [음악] (박수)\n  오늘은  비가 와요  ").unwrap(), "안녕 하세요\n오늘은 비가 와요");
        assert_eq!(clean_transcript("[BLANK_AUDIO]\n").unwrap_err(), "말소리를 찾지 못했습니다");
        assert!(clean_transcript("  \n ").is_err());
        // 말 중간의 괄호는 그대로 둔다.
        assert_eq!(clean_transcript("나는 (진짜) 좋아").unwrap(), "나는 (진짜) 좋아");
    }

    struct Fixture {
        conn: Mutex<Connection>,
        media: PathBuf,
        obs: String,
    }

    fn fixture() -> Fixture {
        let (conn, _c, _s, obs) = seed();
        Fixture { conn: Mutex::new(conn), media: tmp("media").join("media"), obs }
    }

    fn audio(f: &Fixture) -> String {
        std::fs::create_dir_all(&f.media).unwrap();
        attachments::add_bytes(&f.conn.lock().unwrap(), &f.media, &f.obs, "audio", "audio/webm", b"abc").unwrap().id
    }

    fn queue_one(f: &Fixture, id: &str) {
        prepare_job(&f.conn.lock().unwrap(), &f.media, id, &wav(16000, 1, 16, 3200)).unwrap();
    }

    fn get(f: &Fixture, id: &str) -> attachments::Attachment {
        attachments::get(&f.conn.lock().unwrap(), &f.media, id).unwrap()
    }

    #[test]
    fn job_success_saves_transcript_and_removes_temp_wav() {
        let f = fixture();
        let id = audio(&f);
        queue_one(&f, &id);
        let tmp = tmp_dir(&f.media);
        assert!(tmp_wav(&tmp, &id).exists());
        assert_eq!(get(&f, &id).transcript_status, "queued");

        let events = RefCell::new(Vec::<String>::new());
        let engine = |model: &Path, wav: &Path| -> Result<String, String> {
            assert_eq!(model, Path::new("m.bin"));
            assert!(wav.exists());
            // 엔진이 도는 동안 DB 잠금이 풀려 있어야 한다.
            assert_eq!(attachments::get(&f.conn.try_lock().unwrap(), &f.media, &id).unwrap().transcript_status, "running");
            Ok("[BLANK_AUDIO]\n  비가   와요  ".into())
        };
        process_job(&f.conn, &tmp, &id, "whisper-small-q5", Path::new("m.bin"), &engine, &|i, s| events.borrow_mut().push(format!("{i}:{s}")));

        let a = get(&f, &id);
        assert_eq!(a.transcript.as_deref(), Some("비가 와요"));
        assert_eq!((a.transcript_status.as_str(), a.transcript_edited), ("done", false));
        assert_eq!(a.transcript_model.as_deref(), Some("whisper-small-q5"));
        let at: Option<String> = f.conn.lock().unwrap().query_row("SELECT transcribed_at FROM attachments WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert!(at.is_some());
        assert!(!tmp_wav(&tmp, &id).exists());
        assert_eq!(*events.borrow(), vec![format!("{id}:running"), format!("{id}:done")]);
    }

    #[test]
    fn job_failure_and_empty_result_mark_failed() {
        let f = fixture();
        let tmp = tmp_dir(&f.media);
        let id = audio(&f);
        queue_one(&f, &id);
        let boom = |_: &Path, _: &Path| -> Result<String, String> { Err("종료 코드 Some(1)\nbad model".into()) };
        process_job(&f.conn, &tmp, &id, "m", Path::new("m"), &boom, &|_, _| {});
        let a = get(&f, &id);
        assert_eq!(a.transcript_status, "failed");
        assert!(a.transcript_error.unwrap().contains("bad model"));
        assert!(!tmp_wav(&tmp, &id).exists());

        queue_one(&f, &id);
        let silent = |_: &Path, _: &Path| -> Result<String, String> { Ok("[BLANK_AUDIO]".into()) };
        process_job(&f.conn, &tmp, &id, "m", Path::new("m"), &silent, &|_, _| {});
        let a = get(&f, &id);
        assert_eq!((a.transcript_status.as_str(), a.transcript_error.as_deref()), ("failed", Some("말소리를 찾지 못했습니다")));
        assert!(a.transcript.is_none());
    }

    #[test]
    fn queue_processes_in_order_and_skips_deleted() {
        let f = fixture();
        let tmp = tmp_dir(&f.media);
        let (a, b, gone) = (audio(&f), audio(&f), audio(&f));
        for id in [&a, &b, &gone] {
            queue_one(&f, id);
        }
        attachments::delete(&f.conn.lock().unwrap(), &f.media, &gone).unwrap();

        let (tx, rx) = channel();
        for id in [&a, &b, &gone] {
            tx.send(id.clone()).unwrap();
        }
        drop(tx);
        let seen = RefCell::new(Vec::<String>::new());
        let engine = |_: &Path, wav: &Path| -> Result<String, String> {
            let stem = wav.file_stem().unwrap().to_string_lossy().into_owned();
            seen.borrow_mut().push(stem.clone());
            Ok(format!("자막 {stem}"))
        };
        run_queue(rx, &f.conn, &tmp, &|| ("m".to_string(), PathBuf::from("m")), &engine, &|_, _| {});
        assert_eq!(*seen.borrow(), vec![a.clone(), b.clone()]);
        assert_eq!(get(&f, &a).transcript.unwrap(), format!("자막 {a}"));
        assert_eq!(get(&f, &b).transcript_status, "done");
        assert!(!tmp_wav(&tmp, &gone).exists());
    }

    #[test]
    fn prepare_rejects_photo_missing_busy_and_bad_wav() {
        let f = fixture();
        let good = wav(16000, 1, 16, 3200);
        assert!(prepare_job(&f.conn.lock().unwrap(), &f.media, "nope", &good).is_err());

        let src = tmp("photo").join("a.png");
        std::fs::write(&src, b"img").unwrap();
        std::fs::create_dir_all(&f.media).unwrap();
        let p = attachments::add_file(&f.conn.lock().unwrap(), &f.media, &f.obs, src.to_str().unwrap()).unwrap();
        assert!(prepare_job(&f.conn.lock().unwrap(), &f.media, &p.id, &good).unwrap_err().contains("음성"));

        let id = audio(&f);
        assert!(prepare_job(&f.conn.lock().unwrap(), &f.media, &id, &wav(44100, 1, 16, 3200)).is_err());
        assert_eq!(get(&f, &id).transcript_status, "none");
        assert!(!tmp_wav(&tmp_dir(&f.media), &id).exists());
        queue_one(&f, &id);
        assert!(prepare_job(&f.conn.lock().unwrap(), &f.media, &id, &good).unwrap_err().contains("이미"));
        assert_eq!(queue_length(&f.conn.lock().unwrap()).unwrap(), 1);
    }

    #[test]
    fn startup_resets_queued_and_running() {
        let f = fixture();
        let (a, b, c) = (audio(&f), audio(&f), audio(&f));
        {
            let conn = f.conn.lock().unwrap();
            conn.execute("UPDATE attachments SET transcript_status = 'queued' WHERE id = ?1", [&a]).unwrap();
            conn.execute("UPDATE attachments SET transcript_status = 'running' WHERE id = ?1", [&b]).unwrap();
            conn.execute("UPDATE attachments SET transcript_status = 'done', transcript = 't' WHERE id = ?1", [&c]).unwrap();
            assert_eq!(reset_pending(&conn).unwrap(), 2);
        }
        assert_eq!(get(&f, &a).transcript_status, "none");
        assert_eq!(get(&f, &b).transcript_status, "none");
        assert_eq!(get(&f, &c).transcript_status, "done");
    }

    #[test]
    fn settings_defaults() {
        let conn = crate::db::test_conn();
        assert_eq!(model_id_setting(&conn), MODEL_DEFAULT);
        assert!(auto_setting(&conn));
        settings::set(&conn, "sttModelId", &serde_json::json!("whisper-base-q5")).unwrap();
        settings::set(&conn, "sttAuto", &serde_json::json!(false)).unwrap();
        assert_eq!(model_id_setting(&conn), MODEL_LIGHT);
        assert!(!auto_setting(&conn));
        settings::set(&conn, "sttModelId", &serde_json::json!("whisper-base")).unwrap();
        assert_eq!(model_id_setting(&conn), MODEL_DEFAULT);
    }
}
