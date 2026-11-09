//! 행동특성 및 종합의견의 언어 모델 생성기(설계 24). 서버·JSON 스키마(근거 id enum)·실패 처리는 평어(`llm.rs`)와 같다.
//! 프롬프트는 `data/evaluation/behavior-prompt.json`에 둔다. 요청에는 근거 묶음(학생은 「학생」, 다른 아이는 「친구」)만 들어간다.
//!
//! 학기 나누기를 켜면 1학기·2학기 근거를 따로 묶어 서버를 한 번씩 부르고, 각 덩어리 앞에 「(1학기)」「(2학기)」를 붙인다.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

use super::behavior::{self, check_behavior, generate_draft, generation_bundle, prefixed, prune_pairs, remove_semester_labels, semester_label, semester_of, BehaviorView, GenerateInput, Key, NO_CHANGE};
use super::bundle::{Bundle, Evidence};
use super::check::NO_EVIDENCE;
use super::generate::{Sentence, Style};
use super::llm::{load_model_id, seed_for, Chat, FALLBACK_NOTICE, MODEL_MISSING};
use super::{load_settings, normalize_sentences_max, other_names, EvalSettings};
use crate::db::{lock, DbResult, MediaDir};
use crate::diag::download::is_ready;
use crate::diag::{model_path, model_spec};
use crate::engine::engine;
use crate::llm_server::{chat_once, LlmError, LlmServer};

const TEMPERATURE: f64 = 0.4;
const MAX_TOKENS: u32 = 900;
const ATTEMPTS: u32 = 2;
const MAX_CHUNK_SENTENCES: usize = 8;
const PROMPT_JSON: &str = include_str!("../../../../data/evaluation/behavior-prompt.json");

// ---------- 프롬프트 데이터 ----------

#[derive(Debug, Deserialize)]
pub struct Headings {
    pub rules: String,
    pub style: String,
    pub length: String,
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct SystemPrompt {
    pub role: String,
    pub headings: Headings,
    pub rules: Vec<String>,
    pub styles: BTreeMap<String, String>,
    pub length: String,
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct Example {
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
    P.get_or_init(|| serde_json::from_str(PROMPT_JSON).expect("data/evaluation/behavior-prompt.json"))
}

fn style_key(style: Style) -> &'static str {
    match style {
        Style::Ham => "ham",
        Style::Eum => "eum",
        Style::Boim => "boim",
    }
}

pub fn system_prompt(style: Style, target_chars: usize) -> String {
    let s = &prompt().system;
    let h = &s.headings;
    let mut out = format!("{}\n\n[{}]\n", s.role, h.rules);
    for r in &s.rules {
        out.push_str(&format!("- {r}\n"));
    }
    out.push_str(&format!("\n[{}]\n{}\n", h.style, s.styles[style_key(style)]));
    let length = s.length.replace("{target}", &target_chars.to_string()).replace("{min}", &(target_chars * 8 / 10).to_string()).replace("{max}", &(target_chars * 12 / 10).to_string());
    out.push_str(&format!("\n[{}]\n{}\n", h.length, length));
    out.push_str(&format!("\n[{}]\n{}", h.output, s.output));
    out
}

fn user_message(bundle: &Bundle) -> String {
    prompt().user.replace("{bundle}", &serde_json::to_string(bundle).expect("JSON"))
}

/// 응답 JSON 스키마: 근거 id는 이 묶음의 id만 고를 수 있다.
pub fn response_schema(bundle: &Bundle, min_items: usize) -> Value {
    let ids: Vec<&str> = bundle.evidence.iter().map(Evidence::id).collect();
    json!({
        "type": "object",
        "required": ["sentences"],
        "additionalProperties": false,
        "properties": {
            "sentences": {
                "type": "array", "minItems": min_items, "maxItems": MAX_CHUNK_SENTENCES,
                "items": {
                    "type": "object", "required": ["text", "evidence"], "additionalProperties": false,
                    "properties": {
                        "text": { "type": "string", "minLength": 15, "maxLength": 200 },
                        "evidence": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "enum": ids } }
                    }
                }
            }
        }
    })
}

/// 요청 본문. 예시 대화에는 확인 표시(`reviewed`)를 넣지 않는다.
pub fn request_body(bundle: &Bundle, style: Style, target_chars: usize, seed: u32, min_items: usize) -> Value {
    let key = style_key(style);
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(style, target_chars) })];
    for ex in &prompt().examples {
        if let Some(sentences) = ex.outputs.get(key) {
            let shown: Vec<Value> = sentences.iter().map(|s| json!({ "text": s.text, "evidence": s.evidence })).collect();
            messages.push(json!({ "role": "user", "content": user_message(&ex.bundle) }));
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
        "chat_template_kwargs": { "enable_thinking": false },
        "response_format": { "type": "json_schema", "json_schema": { "name": "behavior_draft", "strict": true, "schema": response_schema(bundle, min_items) } },
    })
}

pub fn parse_output(raw: &str) -> Result<Vec<Sentence>, String> {
    if raw.contains("<think>") || raw.contains("</think>") {
        return Err("출력에 생각 과정(<think>)이 섞여 있습니다".to_string());
    }
    #[derive(Deserialize)]
    struct Out {
        sentences: Vec<Sentence>,
    }
    let out: Out = serde_json::from_str(raw.trim()).map_err(|e| format!("출력을 읽을 수 없습니다: {e}"))?;
    if out.sentences.is_empty() || out.sentences.len() > MAX_CHUNK_SENTENCES {
        return Err(format!("문장 수가 맞지 않습니다({}개)", out.sentences.len()));
    }
    normalize_sentences_max(&out.sentences, MAX_CHUNK_SENTENCES)
}

// ---------- 작업 ----------

pub struct Chunk {
    /// 「(1학기)」 같은 학기 표시. 학기를 나누지 않으면 없다.
    pub label: Option<&'static str>,
    pub bundle: Bundle,
    pub target: usize,
}

pub struct Job {
    key: Key,
    split: bool,
    pub chunks: Vec<Chunk>,
    settings: EvalSettings,
    other_names: Vec<String>,
    pub model_id: String,
}

pub enum Outcome {
    Llm(Vec<Sentence>),
    Fallback { detail: String },
}

fn chunk_bundle(b: &Bundle, sem: u8) -> Bundle {
    let mut c = b.clone();
    c.evidence.retain(|e| match e {
        Evidence::Observation { date, .. } => semester_of(date) == sem,
        Evidence::Habit { .. } => sem == 2,
        Evidence::Session { .. } => false,
    });
    prune_pairs(&mut c);
    c
}

pub fn prepare(conn: &Connection, media: &std::path::Path, input: &GenerateInput, model_id: &str) -> DbResult<Job> {
    let k = behavior::norm(&input.key)?;
    let b = generation_bundle(conn, media, &k, &input.excluded_ids)?;
    let settings = load_settings(conn)?;
    let target = behavior::load_target(conn)?;
    let chunks = if input.split_semesters {
        [1u8, 2]
            .into_iter()
            .map(|sem| Chunk { label: Some(semester_label(sem)), bundle: chunk_bundle(&b, sem), target: target / 2 })
            .filter(|c| !c.bundle.evidence.is_empty())
            .collect()
    } else {
        vec![Chunk { label: None, bundle: b, target }]
    };
    let other = other_names(conn, &k.student_id)?;
    Ok(Job { key: k, split: input.split_semesters, chunks, settings, other_names: other, model_id: model_id.to_string() })
}

/// 근거 없음이나 단점의 변화 가능성 누락이 있으면 쓸 수 없는 결과로 본다.
fn accept(job: &Job, chunk: &Chunk, sentences: &[Sentence]) -> Result<(), String> {
    let c = check_behavior(sentences, &chunk.bundle, &job.settings.extra_forbidden, &job.other_names);
    for (i, w) in c.sentences.iter().enumerate() {
        if w.iter().any(|x| x.code == NO_EVIDENCE) {
            return Err(format!("{}번째 문장에 근거가 없습니다", i + 1));
        }
        if w.iter().any(|x| x.code == NO_CHANGE) {
            return Err(format!("{}번째 문장이 단점만 쓰고 변화 가능성이 없습니다", i + 1));
        }
    }
    Ok(())
}

pub fn run(job: &Job, chat: Chat<'_>, cancel: &AtomicBool) -> Result<Outcome, LlmError> {
    let style = Style::parse(&job.settings.style).unwrap_or(Style::Ham);
    let mut all: Vec<Sentence> = Vec::new();
    for (index, chunk) in job.chunks.iter().enumerate() {
        let min_items = if job.split { 1 } else { 2 };
        let mut done: Option<Vec<Sentence>> = None;
        let mut detail = String::new();
        for attempt in 0..ATTEMPTS {
            if cancel.load(Ordering::SeqCst) {
                return Err(LlmError::Cancelled);
            }
            let body = request_body(&chunk.bundle, style, chunk.target, seed_for(&job.key.student_id, attempt) + 1000 * index as u32, min_items);
            match chat(&body) {
                Err(LlmError::Cancelled) => return Err(LlmError::Cancelled),
                Err(LlmError::Unavailable(m)) => return Ok(Outcome::Fallback { detail: m }),
                Err(LlmError::Failed(m)) => detail = m,
                Ok(raw) => match parse_output(&raw).and_then(|s| accept(job, chunk, &s).map(|()| s)) {
                    Ok(s) => {
                        done = Some(s);
                        break;
                    }
                    Err(m) => detail = m,
                },
            }
        }
        let Some(mut sentences) = done else { return Ok(Outcome::Fallback { detail }) };
        if job.split {
            sentences.iter_mut().for_each(|s| s.text = remove_semester_labels(&s.text));
        }
        if let (Some(label), Some(first)) = (chunk.label, sentences.first_mut()) {
            first.text = prefixed(label, &first.text);
        }
        all.extend(sentences);
    }
    Ok(Outcome::Llm(all))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorLlmResult {
    pub view: BehaviorView,
    /// `llm:<모델 id>` 또는 `template`
    pub generator: String,
    pub fell_back: bool,
    pub notice: Option<String>,
    pub detail: Option<String>,
}

pub fn finish(conn: &Connection, media: &std::path::Path, input: &GenerateInput, job: Job, outcome: Outcome) -> DbResult<BehaviorLlmResult> {
    match outcome {
        Outcome::Llm(sentences) => {
            let generator = format!("llm:{}", job.model_id);
            behavior::write_generated(conn, &job.key, job.split, &sentences, &generator)?;
            Ok(BehaviorLlmResult { view: behavior::view(conn, media, &input.key)?, generator, fell_back: false, notice: None, detail: None })
        }
        Outcome::Fallback { detail } => {
            let v = generate_draft(conn, media, input)?;
            Ok(BehaviorLlmResult { view: v, generator: "template".to_string(), fell_back: true, notice: Some(FALLBACK_NOTICE.to_string()), detail: Some(detail) })
        }
    }
}

/// 세 단계를 이어서 실행한다(테스트용).
#[cfg(test)]
pub fn generate_llm_draft(conn: &Connection, media: &std::path::Path, input: &GenerateInput, model_id: &str, chat: Chat<'_>, cancel: &AtomicBool) -> DbResult<BehaviorLlmResult> {
    let job = prepare(conn, media, input, model_id)?;
    let outcome = run(&job, chat, cancel).map_err(|e| e.message())?;
    finish(conn, media, input, job, outcome)
}

// ---------- 명령 ----------

/// 언어 모델로 초안을 만든다. 서버 호출은 DB 잠금 밖에서 하고, 실패하면 템플릿 초안으로 대신한다.
#[tauri::command]
pub async fn eval_behavior_generate_llm(
    app: AppHandle,
    db: State<'_, Mutex<Connection>>,
    media: State<'_, MediaDir>,
    server: State<'_, Arc<LlmServer>>,
    input: GenerateInput,
) -> DbResult<BehaviorLlmResult> {
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
    let log = crate::paths::local_data_dir(&app).ok().map(|d| d.join("logs").join("llama-server.log"));
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
