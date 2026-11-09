//! (2') 언어 모델 생성기: 템플릿 생성기와 같은 입력(`Bundle`)·같은 출력(`Sentence`).
//! 설계: `docs/design/09-local-llm-drafts.md`.
//!
//! 흐름: `prepare`(DB 잠금 안, 근거 묶음 만들기) -> `run`(서버 호출, 잠금 밖) -> `finish`(DB 잠금 안, 저장).
//! 서버 호출은 `Chat` 함수로 주입받아 테스트에서는 가짜를 쓴다. 한국어 프롬프트는 `data/evaluation/prompt.json`에 둔다.
//! 이름 보호: 요청에는 근거 묶음(학생은 「학생」, 다른 아이는 「친구」로 바뀐 것)만 들어간다.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

use super::bundle::{self, Bundle, Evidence, MIN_EVIDENCE};
use super::check::{check_draft, NO_EVIDENCE};
use super::data::forbidden;
use super::generate::{hash, Sentence, Style};
use super::{generate_draft, insufficient_message, load_settings, norm, normalize_sentences, other_names, view, write_draft, DraftView, EvalSettings, GenerateInput, Key};
use crate::db::{lock, settings, DbResult, MediaDir};
use crate::diag::download::is_ready;
use crate::diag::{model_path, model_spec, ModelKind, EVAL_MODEL_DEFAULT};
use crate::engine::engine;
use crate::llm_server::{chat_once, LlmError, LlmServer};

pub const KEY_MODEL: &str = "evalModelId";
pub const FALLBACK_NOTICE: &str = "언어 모델 초안을 만들지 못해 템플릿 초안을 보여 줍니다";
pub const MODEL_MISSING: &str = "평어 초안 모델이 없습니다. 「기기 점검」 탭에서 내려받아 주세요";
const TEMPERATURE: f64 = 0.4;
const MAX_TOKENS: u32 = 700;
/// 처음 한 번, 실패하면 seed+1로 한 번 더.
const ATTEMPTS: u32 = 2;
const MAX_SENTENCES: usize = 4;
const PROMPT_JSON: &str = include_str!("../../../../data/evaluation/prompt.json");

// ---------- 프롬프트 데이터 ----------

#[derive(Debug, Deserialize)]
pub struct Headings {
    pub rules: String,
    pub style: String,
    pub subject: String,
    pub length: String,
    pub standards: String,
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct SystemPrompt {
    pub role: String,
    pub headings: Headings,
    pub rules: Vec<String>,
    pub styles: BTreeMap<String, String>,
    pub subjects: BTreeMap<String, String>,
    pub length: String,
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct Example {
    /// 검증 스크립트가 메시지에 쓰는 이름(앱 코드는 읽지 않는다).
    #[allow(dead_code)]
    pub id: String,
    pub bundle: Bundle,
    pub outputs: BTreeMap<String, Vec<Sentence>>,
}

#[derive(Debug, Deserialize)]
pub struct Prompt {
    pub system: SystemPrompt,
    pub user: String,
    pub examples: Vec<Example>,
}

pub fn prompt() -> &'static Prompt {
    static P: OnceLock<Prompt> = OnceLock::new();
    P.get_or_init(|| serde_json::from_str(PROMPT_JSON).expect("data/evaluation/prompt.json"))
}

fn style_key(style: Style) -> &'static str {
    match style {
        Style::Ham => "ham",
        Style::Eum => "eum",
        Style::Boim => "boim",
    }
}

/// 시스템 프롬프트: 역할, 원칙, 문체, 교과의 성격, 분량, 이 묶음의 성취기준 원문, 출력 형식.
pub fn system_prompt(subject: &str, style: Style, target_chars: usize, standards: &BTreeMap<String, String>) -> String {
    let s = &prompt().system;
    let h = &s.headings;
    let mut out = format!("{}\n\n[{}]\n", s.role, h.rules);
    for r in &s.rules {
        out.push_str(&format!("- {r}\n"));
    }
    out.push_str(&format!("\n[{}]\n{}\n", h.style, s.styles[style_key(style)]));
    out.push_str(&format!("\n[{}]\n{}\n", h.subject, s.subjects.get(subject).map(String::as_str).unwrap_or("")));
    let length = s.length.replace("{target}", &target_chars.to_string()).replace("{min}", &(target_chars * 8 / 10).to_string()).replace("{max}", &(target_chars * 12 / 10).to_string());
    out.push_str(&format!("\n[{}]\n{}\n", h.length, length));
    if !standards.is_empty() {
        out.push_str(&format!("\n[{}]\n", h.standards));
        for (code, text) in standards {
            out.push_str(&format!("- {code} {text}\n"));
        }
    }
    out.push_str(&format!("\n[{}]\n{}", h.output, s.output));
    out
}

fn user_message(bundle: &Bundle) -> String {
    prompt().user.replace("{bundle}", &serde_json::to_string(bundle).expect("JSON"))
}

/// 응답 JSON 스키마: 근거 id는 이 묶음의 id만 고를 수 있다(문법 제약).
pub fn response_schema(bundle: &Bundle) -> Value {
    let ids: Vec<&str> = bundle.evidence.iter().map(Evidence::id).collect();
    json!({
        "type": "object",
        "required": ["sentences"],
        "additionalProperties": false,
        "properties": {
            "sentences": {
                "type": "array", "minItems": 2, "maxItems": MAX_SENTENCES,
                "items": {
                    "type": "object", "required": ["text", "evidence"], "additionalProperties": false,
                    "properties": {
                        "text": { "type": "string", "minLength": 15, "maxLength": 140 },
                        "evidence": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "enum": ids } }
                    }
                }
            }
        }
    })
}

/// 같은 학생이면 같은 seed. 다시 시도할 때는 `attempt`만큼 더한다.
pub fn seed_for(student_id: &str, attempt: u32) -> u32 {
    ((hash(&[student_id]) & 0x3fff_ffff) as u32) + attempt
}

// ---------- 작업 ----------

pub struct Job {
    key: Key,
    pub bundle: Bundle,
    settings: EvalSettings,
    other_names: Vec<String>,
    pub model_id: String,
}

pub enum Outcome {
    Llm(Vec<Sentence>),
    /// 두 번 모두 쓸 수 없는 결과였거나 서버를 쓸 수 없음. `detail`은 마지막 이유.
    Fallback { detail: String },
}

/// 서버로 요청 본문을 보내 첫 메시지의 내용을 받는 함수(테스트에서는 가짜).
pub type Chat<'a> = &'a dyn Fn(&Value) -> Result<String, LlmError>;

/// 요청 본문. `attempt`는 0부터.
pub fn build_request(job: &Job, attempt: u32) -> Value {
    let style = Style::parse(&job.settings.style).unwrap_or(Style::Ham);
    request_body(&job.bundle, style, job.settings.target_chars, seed_for(&job.key.student_id, attempt))
}

/// 묶음 하나로 요청 본문을 만든다(`build_request`와 엔진 통합 테스트가 함께 쓴다).
pub fn request_body(bundle: &Bundle, style: Style, target_chars: usize, seed: u32) -> Value {
    let key = style_key(style);
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(&bundle.subject, style, target_chars, &bundle.standards_text) })];
    for ex in &prompt().examples {
        if let Some(sentences) = ex.outputs.get(key) {
            messages.push(json!({ "role": "user", "content": user_message(&ex.bundle) }));
            // 확인 표시(`reviewed`)는 모델에게 보이지 않는다.
            let shown: Vec<Value> = sentences.iter().map(|s| json!({ "text": s.text, "evidence": s.evidence })).collect();
            messages.push(json!({ "role": "assistant", "content": json!({ "sentences": shown }).to_string() }));
        }
    }
    messages.push(json!({ "role": "user", "content": user_message(bundle) }));
    json!({
        "messages": messages,
        "temperature": TEMPERATURE,
        "seed": seed,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        // 생각(thinking)을 끈다: llama-server의 chat_template_kwargs.enable_thinking (tools/server/server-common.cpp).
        "chat_template_kwargs": { "enable_thinking": false },
        "response_format": { "type": "json_schema", "json_schema": { "name": "pyeongeo_draft", "strict": true, "schema": response_schema(bundle) } },
    })
}

/// 모델 출력(JSON 문자열)을 문장 목록으로 읽는다. `<think>`가 섞였거나 모양이 다르면 오류.
pub fn parse_output(raw: &str) -> Result<Vec<Sentence>, String> {
    if raw.contains("<think>") || raw.contains("</think>") {
        return Err("출력에 생각 과정(<think>)이 섞여 있습니다".to_string());
    }
    #[derive(Deserialize)]
    struct Out {
        sentences: Vec<Sentence>,
    }
    let out: Out = serde_json::from_str(raw.trim()).map_err(|e| format!("출력을 읽을 수 없습니다: {e}"))?;
    if out.sentences.is_empty() || out.sentences.len() > MAX_SENTENCES {
        return Err(format!("문장 수가 맞지 않습니다({}개)", out.sentences.len()));
    }
    normalize_sentences(&out.sentences)
}

/// 설계 08 자동 검사에서 「근거 없음」이 나오면 쓸 수 없는 결과로 본다.
fn accept(job: &Job, sentences: &[Sentence]) -> Result<(), String> {
    let check = check_draft(sentences, &job.bundle, forbidden(), &job.settings.extra_forbidden, &job.other_names);
    match check.sentences.iter().position(|w| w.iter().any(|x| x.code == NO_EVIDENCE)) {
        Some(i) => Err(format!("{}번째 문장에 근거가 없습니다", i + 1)),
        None => Ok(()),
    }
}

/// 서버를 최대 두 번(seed, seed+1) 부른다. 취소는 바로 오류로 돌려준다.
pub fn run(job: &Job, chat: Chat<'_>, cancel: &std::sync::atomic::AtomicBool) -> Result<Outcome, LlmError> {
    let mut detail = String::new();
    for attempt in 0..ATTEMPTS {
        if cancel.load(Ordering::SeqCst) {
            return Err(LlmError::Cancelled);
        }
        match chat(&build_request(job, attempt)) {
            Err(LlmError::Cancelled) => return Err(LlmError::Cancelled),
            Err(LlmError::Unavailable(m)) => return Ok(Outcome::Fallback { detail: m }),
            Err(LlmError::Failed(m)) => detail = m,
            Ok(raw) => match parse_output(&raw).and_then(|s| accept(job, &s).map(|()| s)) {
                Ok(s) => return Ok(Outcome::Llm(s)),
                Err(m) => detail = m,
            },
        }
    }
    Ok(Outcome::Fallback { detail })
}

pub fn prepare(conn: &Connection, media: &std::path::Path, input: &GenerateInput, model_id: &str) -> DbResult<Job> {
    let k = norm(&input.key)?;
    let mut b = bundle::build(conn, media, &k.student_id, k.subject, &k.from, &k.to, &[])?;
    b.evidence.retain(|e| !input.excluded_ids.iter().any(|x| x == e.id()));
    if b.evidence.len() < MIN_EVIDENCE {
        return Err(insufficient_message(b.evidence.len()));
    }
    // 근거에서 빠진 기록의 성취기준 원문은 프롬프트에서도 뺀다.
    let used: Vec<&String> = b
        .evidence
        .iter()
        .flat_map(|e| match e {
            Evidence::Observation { standards, .. } | Evidence::Session { standards, .. } => standards.iter().collect::<Vec<_>>(),
            Evidence::Habit { .. } => vec![],
        })
        .collect();
    b.standards_text.retain(|code, _| used.contains(&code));
    let settings = load_settings(conn)?;
    let other = other_names(conn, &k.student_id)?;
    Ok(Job { key: k, bundle: b, settings, other_names: other, model_id: model_id.to_string() })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmDraftResult {
    pub view: DraftView,
    /// `llm:<모델 id>` 또는 `template`
    pub generator: String,
    pub fell_back: bool,
    pub notice: Option<String>,
    /// 대신 쓴 이유(마지막 실패 내용).
    pub detail: Option<String>,
}

pub fn finish(conn: &Connection, media: &std::path::Path, input: &GenerateInput, job: Job, outcome: Outcome) -> DbResult<LlmDraftResult> {
    match outcome {
        Outcome::Llm(sentences) => {
            let generator = format!("llm:{}", job.model_id);
            write_draft(conn, &job.key.student_id, &job.key, &sentences, &generator)?;
            Ok(LlmDraftResult { view: view(conn, media, &input.key)?, generator, fell_back: false, notice: None, detail: None })
        }
        Outcome::Fallback { detail } => {
            let v = generate_draft(conn, media, input)?;
            Ok(LlmDraftResult { view: v, generator: "template".to_string(), fell_back: true, notice: Some(FALLBACK_NOTICE.to_string()), detail: Some(detail) })
        }
    }
}

/// 세 단계를 이어서 실행한다(DB를 잠근 채로 서버를 부르므로 테스트용).
#[cfg(test)]
pub fn generate_llm_draft(
    conn: &Connection,
    media: &std::path::Path,
    input: &GenerateInput,
    model_id: &str,
    chat: Chat<'_>,
    cancel: &std::sync::atomic::AtomicBool,
) -> DbResult<LlmDraftResult> {
    let job = prepare(conn, media, input, model_id)?;
    let outcome = run(&job, chat, cancel).map_err(|e| e.message())?;
    finish(conn, media, input, job, outcome)
}

// ---------- 모델 설정과 명령 ----------

/// 설정 `evalModelId`. 없거나 언어 모델이 아니면 기본 모델.
pub fn load_model_id(conn: &Connection) -> DbResult<String> {
    let id = settings::get(conn, KEY_MODEL)?;
    Ok(id.as_str().filter(|s| model_spec(s).is_ok_and(|m| m.kind == ModelKind::Llm)).unwrap_or(EVAL_MODEL_DEFAULT).to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmStatus {
    pub model_id: String,
    pub model_label: String,
    pub model_ready: bool,
    pub engine_ready: bool,
    pub running: bool,
}

#[tauri::command]
pub fn eval_llm_status(app: AppHandle, db: State<Mutex<Connection>>, server: State<Arc<LlmServer>>) -> DbResult<LlmStatus> {
    let id = load_model_id(&*lock(&db)?)?;
    let spec = model_spec(&id)?;
    Ok(LlmStatus {
        model_id: id,
        model_label: spec.label.to_string(),
        model_ready: is_ready(&model_path(&app, spec)?, spec),
        engine_ready: engine(&app, "llama", "llama-server").is_ok(),
        running: server.is_running(),
    })
}

/// 화면의 「취소」: 진행 중인 서버 호출을 끊는다.
#[tauri::command]
pub fn eval_llm_cancel(server: State<Arc<LlmServer>>) {
    server.cancel.store(true, Ordering::SeqCst);
}

fn log_path(app: &AppHandle) -> Option<PathBuf> {
    crate::paths::local_data_dir(app).ok().map(|d| d.join("logs").join("llama-server.log"))
}

/// 언어 모델로 초안을 만든다. 서버 호출은 DB 잠금 밖에서 하고, 실패하면 템플릿 초안으로 대신한다.
#[tauri::command]
pub async fn eval_generate_llm(
    app: AppHandle,
    db: State<'_, Mutex<Connection>>,
    media: State<'_, MediaDir>,
    server: State<'_, Arc<LlmServer>>,
    input: GenerateInput,
) -> DbResult<LlmDraftResult> {
    let server: Arc<LlmServer> = server.inner().clone();
    let (job, model, program) = {
        let conn = lock(&db)?;
        let id = load_model_id(&conn)?;
        let spec = model_spec(&id)?;
        let model = model_path(&app, spec)?;
        if !is_ready(&model, spec) {
            return Err(MODEL_MISSING.to_string());
        }
        let program = engine(&app, "llama", "llama-server")?;
        (prepare(&conn, &media.0, &input, &id)?, model, program)
    };
    let log = log_path(&app);
    server.cancel.store(false, Ordering::SeqCst);
    let s2 = server.clone();
    let (job, outcome) = tauri::async_runtime::spawn_blocking(move || {
        let _one = s2.one_at_a_time.lock().unwrap_or_else(|e| e.into_inner());
        let chat = |req: &Value| -> Result<String, LlmError> {
            let _busy = s2.begin();
            let port = s2.ensure(&program, &model, log.clone(), &s2.cancel)?;
            chat_once(port, req, &s2.cancel)
        };
        let outcome = run(&job, &chat, &s2.cancel);
        outcome.map(|o| (job, o))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.message())?;
    finish(&*lock(&db)?, &media.0, &input, job, outcome)
}
