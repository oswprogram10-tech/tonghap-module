//! 엔진 통합 테스트(`src-tauri/tests/engines.rs`)가 앱과 같은 인자·요청 본문을 쓰도록 여는 통로.
//! 앱 화면에서는 쓰지 않는다.

pub use crate::db::evaluation::bundle::{Bundle, BundlePeriod, BundleStudent, Evidence};
pub use crate::db::evaluation::generate::Style;
pub use crate::db::evaluation::llm::request_body;
pub use crate::diag::bench::{llama_bench_args, parse_llama_bench, LLM_TIMEOUT};
pub use crate::engine::{engine_in, run, tail};
pub use crate::llm_server::{chat_once, completion_content, LlmServer};
pub use crate::stt::{clean_transcript, validate_wav, whisper_args, STT_TIMEOUT};
pub use crate::llm_server::{http, LlmError};
