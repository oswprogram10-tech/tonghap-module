//! 뜻 임베딩(multilingual-e5-small): 활동 추천의 뜻 점수와 학생 질문 묶기. 설계: docs/design/22-embedding-search.md
//!
//! - 번들된 `llama-server`를 임베딩 전용으로 따로 띄운다(`--embedding --pooling mean`, CPU). 서버 관리(Job Object, 5분 유휴 종료,
//!   앱 종료 때 정리)는 `llm_server::LlmServer`를 그대로 쓰고 평어 초안 서버와는 따로 둔다.
//! - 요청은 `POST /v1/embeddings`(llama.cpp b11600의 OpenAI 호환 길: 응답이 `data[i].embedding`의 1차원 배열이고 서버가 L2 정규화한다).
//! - 입력 앞에 E5 규칙대로 `query: `(질문) / `passage: `(카드 글)을 붙인다.
//! - 모델이 없거나 서버가 실패하면 호출한 쪽이 규칙 방식으로 대신한다(이 모듈은 오류 문자열만 돌려준다).
//! - 아이 말(학생 질문)은 서버로만 보내고(127.0.0.1) 어디에도 저장하지 않는다.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, State};

use crate::diag::download::is_ready;
use crate::diag::{model_path, model_spec};
use crate::engine::{engine, tail};
use crate::llm_server::{http, spawn_sweeper, LlmError, LlmServer, HOST};

pub const MODEL_ID: &str = "e5-small-q8";
pub const DIM: usize = 384;
pub const QUERY_PREFIX: &str = "query: ";
pub const PASSAGE_PREFIX: &str = "passage: ";
/// 서버의 문맥(`-c 512`)을 넘지 않도록 글자 수를 줄인다(한국어는 대략 한 글자가 한 토큰 안팎).
pub const MAX_PASSAGE_CHARS: usize = 400;
pub const MAX_QUERY_CHARS: usize = 200;
/// 한 요청에 담는 글 수.
const BATCH: usize = 16;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// 질문 묶기에 넣을 수 있는 질문 수.
pub const MAX_CLUSTER_QUESTIONS: usize = 40;
/// 묶기를 하려면 필요한 최소 질문 수(유사도 분포의 사분위를 구하려면 짝이 3쌍은 있어야 한다).
pub const MIN_CLUSTER_QUESTIONS: usize = 3;
pub const MODEL_MISSING: &str = "질문 묶기 모델이 없습니다. 「기기 점검」 탭에서 내려받아 주세요";

/// 글 여러 개를 임베딩하는 함수(테스트에서는 가짜). 입력 순서대로 벡터를 돌려준다.
pub type EmbedFn<'a> = &'a dyn Fn(&[String]) -> Result<Vec<Vec<f32>>, String>;

// ---------- 실행 인자 ----------

/// llama-server 임베딩 인자. 바깥 주소로는 열지 않는다.
/// 임베딩은 입력 전체를 한 번에 처리해야 하므로 `-b`·`-ub`를 문맥과 같게 두고, 슬롯은 하나(`-np 1`)로 한다.
pub fn server_args(model: &Path, port: u16, gpu_layers: u32) -> Vec<String> {
    vec![
        "--host".into(),
        HOST.into(),
        "--port".into(),
        port.to_string(),
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "--embedding".into(),
        "--pooling".into(),
        "mean".into(),
        "-c".into(),
        "512".into(),
        "-b".into(),
        "512".into(),
        "-ub".into(),
        "512".into(),
        "-ngl".into(),
        gpu_layers.to_string(),
        "-np".into(),
        "1".into(),
        "--no-webui".into(),
    ]
}

// ---------- 벡터 계산 ----------

/// 길이가 1이 되게 나눈다(0 벡터는 그대로).
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

/// 코사인 유사도(길이가 다르거나 0 벡터면 0).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// DB에 넣는 모양: f32 리틀엔디언.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// 384차원이 아니면 `None`(깨졌거나 다른 모델의 벡터).
pub fn from_blob(b: &[u8]) -> Option<Vec<f32>> {
    if b.len() != DIM * 4 {
        return None;
    }
    Some(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// 임베딩한 글의 SHA-256 앞 16자리(글이 바뀌었는지 보는 용도).
pub fn text_hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// 앞에서 `max`글자까지.
pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ---------- 서버 요청 ----------

/// `/v1/embeddings` 요청 본문. `encoding_format: float`을 분명히 적는다.
pub fn request_body(texts: &[String]) -> Value {
    json!({ "input": texts, "encoding_format": "float" })
}

/// `/v1/embeddings` 응답에서 `index` 순서대로 벡터를 꺼내 길이를 1로 맞춘다. 개수·차원이 다르면 오류.
pub fn parse_embeddings(body: &str, expected: usize) -> Result<Vec<Vec<f32>>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("임베딩 응답을 읽을 수 없습니다: {e}"))?;
    let data = v["data"].as_array().ok_or("임베딩 응답에 data가 없습니다")?;
    if data.len() != expected {
        return Err(format!("임베딩 응답의 개수가 맞지 않습니다({}개, 기대 {expected}개)", data.len()));
    }
    let mut rows: Vec<(usize, Vec<f32>)> = Vec::with_capacity(expected);
    for (pos, item) in data.iter().enumerate() {
        let index = item["index"].as_u64().map_or(pos, |i| i as usize);
        let arr = item["embedding"].as_array().ok_or("임베딩 응답에 embedding이 없습니다")?;
        let mut vec: Vec<f32> = Vec::with_capacity(arr.len());
        for x in arr {
            vec.push(x.as_f64().ok_or("임베딩 값이 숫자가 아닙니다")? as f32);
        }
        if vec.len() != DIM {
            return Err(format!("임베딩 차원이 {DIM}이 아닙니다({})", vec.len()));
        }
        normalize(&mut vec);
        rows.push((index, vec));
    }
    rows.sort_by_key(|r| r.0);
    if rows.iter().enumerate().any(|(i, r)| r.0 != i) {
        return Err("임베딩 응답의 index가 맞지 않습니다".to_string());
    }
    Ok(rows.into_iter().map(|r| r.1).collect())
}

/// 요청 한 번(서버가 떠 있어야 한다).
pub fn embed_once(port: u16, texts: &[String], cancel: &std::sync::atomic::AtomicBool) -> Result<Vec<Vec<f32>>, LlmError> {
    let (status, body) = http(port, "POST", "/v1/embeddings", Some(&request_body(texts).to_string()), REQUEST_TIMEOUT, cancel)?;
    if status != 200 {
        return Err(LlmError::Failed(format!("임베딩 서버가 오류를 돌려주었습니다({status}): {}", tail(body.trim(), 300))));
    }
    parse_embeddings(&body, texts.len()).map_err(LlmError::Failed)
}

// ---------- 서버 관리 ----------

/// 임베딩 전용 서버 관리자(관리 상태). 평어 초안 서버(`Arc<LlmServer>`)와 따로 둔다.
pub struct EmbedServer(pub Arc<LlmServer>);

impl EmbedServer {
    pub fn new() -> EmbedServer {
        EmbedServer(Arc::new(LlmServer::real().with_args(server_args, &[0])))
    }
}

impl Default for EmbedServer {
    fn default() -> Self {
        Self::new()
    }
}

/// 앱 시작 때 호출: 임베딩 서버 관리자를 관리 상태로 두고 유휴 종료 감시를 띄운다.
pub fn start(app: &AppHandle) {
    use tauri::Manager;
    let server = EmbedServer::new();
    spawn_sweeper(&server.0);
    app.manage(server);
}

/// 임베딩에 쓸 실행 파일과 모델 경로(둘 다 있을 때만).
#[derive(Clone)]
pub struct EnginePaths {
    pub program: PathBuf,
    pub model: PathBuf,
}

pub fn ready(app: &AppHandle) -> Option<EnginePaths> {
    let spec = model_spec(MODEL_ID).ok()?;
    let model = model_path(app, spec).ok()?;
    if !is_ready(&model, spec) {
        return None;
    }
    let program = engine(app, "llama", "llama-server").ok()?;
    Some(EnginePaths { program, model })
}

pub fn log_path(app: &AppHandle) -> Option<PathBuf> {
    crate::paths::local_data_dir(app).ok().map(|d| d.join("logs").join("embed-server.log"))
}

/// 글 여러 개를 임베딩한다: 서버를 띄우고(이미 떠 있으면 그대로) `BATCH`개씩 보낸다. 사용하는 동안은 유휴 종료를 막는다.
pub fn embed_texts(server: &LlmServer, paths: &EnginePaths, log: Option<PathBuf>, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let _one = server.one_at_a_time.lock().unwrap_or_else(|e| e.into_inner());
    server.cancel.store(false, Ordering::SeqCst);
    let _busy = server.begin();
    let port = server.ensure(&paths.program, &paths.model, log, &server.cancel).map_err(|e| e.message())?;
    let mut out = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(BATCH) {
        out.extend(embed_once(port, chunk, &server.cancel).map_err(|e| e.message())?);
    }
    Ok(out)
}

// ---------- 질문 묶기 ----------

/// 묶음 하나: 질문들(입력 순서)과 대표 질문(묶음 중심에 가장 가까운 것).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuestionGroup {
    pub questions: Vec<String>,
    pub representative: String,
}

/// 쌍별 유사도 분포의 상위 사분위(75% 지점, 선형 보간).
/// 고정 문턱을 쓰지 않는 근거: e5는 관련 없는 글끼리도 코사인이 0.7대라 절댓값이 질문 묶음마다 다르다.
/// 대신 이번 질문들끼리의 유사도 분포에서 가장 가까운 4분의 1 쌍을 「비슷하다」고 본다.
pub fn upper_quartile(sims: &[f32]) -> f32 {
    if sims.is_empty() {
        return f32::INFINITY;
    }
    let mut v = sims.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let pos = 0.75 * (v.len() - 1) as f32;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f32)
}

/// 평균 연결 계층 묶기: 묶음 사이 평균 유사도가 가장 큰 두 묶음을 합치고, 그 값이 문턱(쌍별 유사도의 상위 사분위)보다 작아지면 멈춘다.
/// 질문이 `MIN_CLUSTER_QUESTIONS`개보다 적으면 묶지 않는다. 동점이면 앞 묶음부터 합쳐 결과가 늘 같다.
/// 돌려주는 묶음은 큰 것부터(같으면 첫 질문이 앞선 것부터), 묶음 안은 입력 순서.
pub fn cluster(vecs: &[Vec<f32>]) -> Vec<Vec<usize>> {
    let n = vecs.len();
    let mut groups: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    if n >= MIN_CLUSTER_QUESTIONS {
        let sim: Vec<Vec<f32>> = (0..n).map(|i| (0..n).map(|j| cosine(&vecs[i], &vecs[j])).collect()).collect();
        let pairs: Vec<f32> = (0..n).flat_map(|i| ((i + 1)..n).map(move |j| (i, j))).map(|(i, j)| sim[i][j]).collect();
        let threshold = upper_quartile(&pairs);
        let linkage = |a: &[usize], b: &[usize]| -> f32 {
            let mut total = 0.0f32;
            for &i in a {
                for &j in b {
                    total += sim[i][j];
                }
            }
            total / (a.len() * b.len()) as f32
        };
        loop {
            let mut best: Option<(f32, usize, usize)> = None;
            for i in 0..groups.len() {
                for j in (i + 1)..groups.len() {
                    let l = linkage(&groups[i], &groups[j]);
                    if best.map_or(true, |b| l > b.0) {
                        best = Some((l, i, j));
                    }
                }
            }
            match best {
                Some((l, i, j)) if l >= threshold => {
                    let merged = groups.remove(j);
                    groups[i].extend(merged);
                    groups[i].sort_unstable();
                }
                _ => break,
            }
        }
    }
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    groups
}

/// 묶음 중심(벡터 평균)에 코사인이 가장 큰 질문의 번호(동점이면 앞선 것).
pub fn representative(vecs: &[Vec<f32>], members: &[usize]) -> usize {
    let mut centroid = vec![0.0f32; DIM];
    for &m in members {
        for (c, x) in centroid.iter_mut().zip(&vecs[m]) {
            *c += x;
        }
    }
    let mut best = members[0];
    let mut best_sim = f32::NEG_INFINITY;
    for &m in members {
        let s = cosine(&centroid, &vecs[m]);
        if s > best_sim {
            best = m;
            best_sim = s;
        }
    }
    best
}

/// 질문들을 임베딩해 묶는다(`query: ` 접두사). 결과는 저장하지 않는다.
pub fn group_questions(questions: &[String], embed_fn: EmbedFn<'_>) -> Result<Vec<QuestionGroup>, String> {
    if questions.is_empty() {
        return Ok(Vec::new());
    }
    let texts: Vec<String> = questions.iter().map(|q| format!("{QUERY_PREFIX}{}", clip(q, MAX_QUERY_CHARS))).collect();
    let mut vecs = embed_fn(&texts)?;
    if vecs.len() != questions.len() || vecs.iter().any(|v| v.len() != DIM) {
        return Err("임베딩 결과가 맞지 않습니다".to_string());
    }
    vecs.iter_mut().for_each(|v| normalize(v));
    Ok(cluster(&vecs)
        .into_iter()
        .map(|members| QuestionGroup { representative: questions[representative(&vecs, &members)].clone(), questions: members.iter().map(|&i| questions[i].clone()).collect() })
        .collect())
}

// ---------- Tauri 명령 ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedStatus {
    pub model_ready: bool,
    pub engine_ready: bool,
    /// 모델과 엔진이 모두 있어 뜻 기능을 쓸 수 있음.
    pub ready: bool,
}

/// 「비슷한 질문 묶기」 단추를 보일지 정한다.
#[tauri::command]
pub fn embed_status(app: AppHandle) -> EmbedStatus {
    let model_ready = model_spec(MODEL_ID).ok().and_then(|spec| model_path(&app, spec).ok().map(|p| is_ready(&p, spec))).unwrap_or(false);
    let engine_ready = engine(&app, "llama", "llama-server").is_ok();
    EmbedStatus { model_ready, engine_ready, ready: model_ready && engine_ready }
}

#[derive(Debug, Deserialize)]
pub struct ClusterInput {
    pub questions: Vec<String>,
}

/// 학생 질문을 뜻이 비슷한 것끼리 묶는다. 결과는 화면에만 쓰고 저장하지 않는다.
#[tauri::command]
pub async fn question_cluster(app: AppHandle, embed_server: State<'_, EmbedServer>, input: ClusterInput) -> Result<Vec<QuestionGroup>, String> {
    let questions: Vec<String> = input.questions.iter().map(|q| q.trim().to_string()).filter(|q| !q.is_empty()).collect();
    if questions.len() > MAX_CLUSTER_QUESTIONS {
        return Err(format!("질문은 {MAX_CLUSTER_QUESTIONS}개까지 묶을 수 있어요 (지금 {}개)", questions.len()));
    }
    let paths = ready(&app).ok_or(MODEL_MISSING)?;
    let server = Arc::clone(&embed_server.0);
    let log = log_path(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let embed_fn = |texts: &[String]| embed_texts(&server, &paths, log.clone(), texts);
        group_questions(&questions, &embed_fn)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("질문을 묶지 못했습니다. {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::atomic::AtomicBool;

    fn axis(i: usize) -> Vec<f32> {
        let mut v = vec![0.0; DIM];
        v[i] = 1.0;
        v
    }

    fn mix(a: usize, b: usize, wa: f32, wb: f32) -> Vec<f32> {
        let mut v = vec![0.0; DIM];
        v[a] = wa;
        v[b] = wb;
        normalize(&mut v);
        v
    }

    #[test]
    fn args_are_embedding_only_on_localhost_with_mean_pooling() {
        let a = server_args(Path::new("m.gguf"), 41234, 0);
        let at = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].clone());
        assert_eq!(at("--host").as_deref(), Some("127.0.0.1"));
        assert_eq!(at("--port").as_deref(), Some("41234"));
        assert_eq!(at("-m").as_deref(), Some("m.gguf"));
        assert!(a.contains(&"--embedding".to_string()));
        assert_eq!(at("--pooling").as_deref(), Some("mean"));
        assert_eq!(at("-c").as_deref(), Some("512"));
        assert_eq!(at("-ub").as_deref(), Some("512"));
        assert_eq!(at("-ngl").as_deref(), Some("0"));
        assert_eq!(at("-np").as_deref(), Some("1"));
        assert!(!a.iter().any(|x| x == "0.0.0.0" || x == "::" || x == "--jinja"));
    }

    #[test]
    fn normalize_and_cosine() {
        let mut v = vec![3.0, 4.0];
        normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        let mut zero = vec![0.0; 4];
        normalize(&mut zero);
        assert_eq!(zero, vec![0.0; 4]);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!((cosine(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        // 길이가 달라도 방향만 본다.
        assert!((cosine(&[2.0, 0.0], &[5.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn blob_roundtrip_is_little_endian_f32_and_rejects_wrong_size() {
        let v: Vec<f32> = (0..DIM).map(|i| i as f32 * 0.5 - 3.0).collect();
        let b = to_blob(&v);
        assert_eq!(b.len(), DIM * 4);
        assert_eq!(&b[..4], &(-3.0f32).to_le_bytes());
        assert_eq!(from_blob(&b).unwrap(), v);
        assert!(from_blob(&b[..b.len() - 4]).is_none());
        assert!(from_blob(&[]).is_none());
    }

    #[test]
    fn text_hash_is_16_hex_chars_and_changes_with_text() {
        let h = text_hash("passage: 곤충 관찰");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h, text_hash("passage: 곤충 관찰"));
        assert_ne!(h, text_hash("passage: 곤충 관찰!"));
        assert_eq!(text_hash(""), "e3b0c44298fc1c14", "빈 글 SHA-256의 앞 16자리");
    }

    fn response_json(vecs: &[Vec<f32>], order: &[usize]) -> String {
        let data: Vec<Value> = order.iter().map(|&i| json!({ "embedding": vecs[i], "index": i, "object": "embedding" })).collect();
        json!({ "model": "e5", "object": "list", "usage": { "prompt_tokens": 3, "total_tokens": 3 }, "data": data }).to_string()
    }

    #[test]
    fn parses_the_openai_compatible_response_sorted_by_index_and_normalized() {
        let vecs = vec![axis(1), axis(2)];
        let scaled: Vec<Vec<f32>> = vecs.iter().map(|v| v.iter().map(|x| x * 5.0).collect()).collect();
        let body = response_json(&scaled, &[1, 0]);
        let out = parse_embeddings(&body, 2).unwrap();
        assert_eq!(out, vecs, "index 순서로 돌려주고 길이를 1로 맞춘다");
        // llama.cpp가 실제로 돌려주는 필드(tokens_evaluated 등)가 더 있어도 읽는다.
        let extra = format!(r#"{{"object":"list","data":[{{"embedding":{},"index":0,"object":"embedding"}}],"usage":{{}}}}"#, serde_json::to_string(&axis(0)).unwrap());
        assert_eq!(parse_embeddings(&extra, 1).unwrap(), vec![axis(0)]);
    }

    #[test]
    fn rejects_bad_embedding_responses() {
        assert!(parse_embeddings("not json", 1).is_err());
        assert!(parse_embeddings("{}", 1).is_err());
        assert!(parse_embeddings(&response_json(&[axis(0)], &[0]), 2).unwrap_err().contains("개수"));
        let short = json!({ "data": [{ "embedding": [0.1, 0.2], "index": 0 }] }).to_string();
        assert!(parse_embeddings(&short, 1).unwrap_err().contains("384"));
        let text = json!({ "data": [{ "embedding": vec!["x"; DIM], "index": 0 }] }).to_string();
        assert!(parse_embeddings(&text, 1).is_err());
        // 법정 모양이 아닌 비-OAI 응답(`/embedding`의 중첩 배열)은 받지 않는다: 우리는 /v1/embeddings만 쓴다.
        let nested = json!([{ "index": 0, "embedding": [axis(0)] }]).to_string();
        assert!(parse_embeddings(&nested, 1).is_err());
    }

    #[test]
    fn request_body_has_input_array_and_float_format() {
        let b = request_body(&["query: 가".to_string(), "passage: 나".to_string()]);
        assert_eq!(b["input"], json!(["query: 가", "passage: 나"]));
        assert_eq!(b["encoding_format"], "float");
    }

    /// 한 번 응답하고 끝나는 가짜 서버(진짜 소켓). 받은 요청 전체를 돌려준다.
    fn fake_server(response: String) -> (u16, std::thread::JoinHandle<String>) {
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
            }
            let _ = s.write_all(response.as_bytes());
            String::from_utf8_lossy(&got).into_owned()
        });
        (port, h)
    }

    #[test]
    fn embed_once_posts_to_v1_embeddings_on_localhost() {
        let body = response_json(&[axis(0), axis(1)], &[0, 1]);
        let (port, h) = fake_server(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body));
        let texts = vec!["query: 개미".to_string(), "passage: 곤충".to_string()];
        let out = embed_once(port, &texts, &AtomicBool::new(false)).unwrap();
        assert_eq!(out, vec![axis(0), axis(1)]);
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /v1/embeddings HTTP/1.1"), "{sent}");
        assert!(sent.contains("Host: 127.0.0.1:"));
        assert!(sent.contains(r#""input":["query: 개미","passage: 곤충"]"#), "{sent}");
    }

    #[test]
    fn embed_once_reports_server_errors() {
        let (port, h) = fake_server("HTTP/1.1 400 Bad Request\r\nContent-Length: 20\r\n\r\ninput is too large!!".to_string());
        let e = embed_once(port, &["query: x".to_string()], &AtomicBool::new(false)).unwrap_err();
        assert!(matches!(e, LlmError::Failed(m) if m.contains("400") && m.contains("too large")));
        h.join().unwrap();
    }

    #[test]
    fn upper_quartile_interpolates_and_handles_edges() {
        assert_eq!(upper_quartile(&[0.5]), 0.5);
        assert!((upper_quartile(&[0.0, 1.0]) - 0.75).abs() < 1e-6);
        assert!((upper_quartile(&[4.0, 0.0, 1.0, 2.0, 3.0]) - 3.0).abs() < 1e-6);
        assert!(upper_quartile(&[]).is_infinite());
    }

    #[test]
    fn clusters_similar_vectors_and_keeps_unrelated_ones_apart() {
        // 0,1,2는 축 10 근처(개미 이야기), 3,4는 축 20 근처(태극기), 5는 혼자.
        let vecs = vec![mix(10, 11, 1.0, 0.1), mix(10, 12, 1.0, 0.1), mix(10, 13, 1.0, 0.15), mix(20, 21, 1.0, 0.1), mix(20, 22, 1.0, 0.1), axis(30)];
        let groups = cluster(&vecs);
        assert_eq!(groups, vec![vec![0, 1, 2], vec![3, 4], vec![5]]);
    }

    #[test]
    fn cluster_threshold_follows_the_distribution_not_a_fixed_number() {
        // 같은 모양(같은 순위)의 유사도를 전체적으로 끌어올려도(e5처럼 모두 높은 경우) 같은 묶음이 나온다.
        let spread = |base: f32| vec![mix(1, 2, 1.0, base), mix(1, 3, 1.0, base), axis(7), axis(8), mix(1, 4, 1.0, base)];
        let low = cluster(&spread(0.1));
        // 모든 벡터에 공통 성분(축 0)을 크게 섞어 코사인을 전반적으로 높인다.
        let lift = |v: &Vec<f32>| {
            let mut w = v.clone();
            w[0] = 4.0;
            normalize(&mut w);
            w
        };
        let high_vecs: Vec<Vec<f32>> = spread(0.1).iter().map(lift).collect();
        let all_high = (0..5).all(|i| (0..5).filter(|&j| j != i).all(|j| cosine(&high_vecs[i], &high_vecs[j]) > 0.9));
        assert!(all_high, "공통 성분 때문에 모든 쌍이 매우 비슷해야 한다");
        assert_eq!(cluster(&high_vecs), low);
        assert_eq!(low[0], vec![0, 1, 4]);
    }

    #[test]
    fn too_few_questions_are_not_clustered_and_empty_is_fine() {
        assert!(cluster(&[]).is_empty());
        assert_eq!(cluster(&[axis(0)]), vec![vec![0]]);
        assert_eq!(cluster(&[axis(0), axis(0)]), vec![vec![0], vec![1]]);
    }

    #[test]
    fn cluster_is_deterministic_including_ties() {
        let vecs = vec![axis(1), axis(1), axis(1), axis(2), axis(2), axis(2)];
        let first = cluster(&vecs);
        assert_eq!(first, vec![vec![0, 1, 2], vec![3, 4, 5]]);
        for _ in 0..5 {
            assert_eq!(cluster(&vecs), first);
        }
    }

    #[test]
    fn representative_is_the_member_closest_to_the_center() {
        let vecs = vec![mix(1, 2, 1.0, 1.0), axis(1), mix(1, 3, 1.0, 1.0)];
        // 0번이 축 1·2의 가운데가 아니라, 세 벡터의 평균(축 1이 가장 큼)에 가장 가까운 것은 1번이다.
        assert_eq!(representative(&vecs, &[0, 1, 2]), 1);
        assert_eq!(representative(&vecs, &[2]), 2);
    }

    #[test]
    fn group_questions_prefixes_queries_and_returns_groups_with_representatives() {
        let questions: Vec<String> = ["개미는 왜 줄 서요?", "개미집은 어디 있어요?", "개미는 뭘 먹어요?", "태극기는 왜 네모예요?", "태극기 색깔은 왜 달라요?", "비 오면 뭐 해요?"].iter().map(|s| s.to_string()).collect();
        let seen = std::cell::RefCell::new(Vec::<String>::new());
        let embed_fn = |texts: &[String]| -> Result<Vec<Vec<f32>>, String> {
            seen.borrow_mut().extend(texts.iter().cloned());
            Ok(texts
                .iter()
                .map(|t| {
                    if t.contains("개미") {
                        mix(10, 11 + t.chars().count() % 5, 1.0, 0.1)
                    } else if t.contains("태극기") {
                        mix(20, 21 + t.chars().count() % 5, 1.0, 0.1)
                    } else {
                        axis(30)
                    }
                })
                .collect())
        };
        let groups = group_questions(&questions, &embed_fn).unwrap();
        assert!(seen.borrow().iter().all(|t| t.starts_with("query: ")));
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].questions, questions[..3].to_vec());
        assert_eq!(groups[1].questions, questions[3..5].to_vec());
        assert_eq!(groups[2].questions, vec![questions[5].clone()]);
        assert!(groups[0].questions.contains(&groups[0].representative));
        assert_eq!(groups[2].representative, questions[5]);
        assert!(group_questions(&[], &embed_fn).unwrap().is_empty());
        assert!(group_questions(&questions, &|_: &[String]| Err("서버 오류".to_string())).is_err());
        assert!(group_questions(&questions, &|t: &[String]| Ok(vec![axis(0); t.len() - 1])).is_err());
    }
}
