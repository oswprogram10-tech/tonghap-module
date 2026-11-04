# 설계 09: 평어 초안 — 로컬 언어 모델 연결과 합성 데이터 준비

설계 08의 템플릿 생성기 자리에 **같은 입력(근거 묶음)·같은 출력(문장 + 근거 id)**을 쓰는 로컬 언어 모델 생성기를 붙인다.
이번에는 미세조정 없이 공개 모델 그대로 연결하고(예시 몇 개를 프롬프트에 넣는 방식), 미세조정에 쓸 **합성 근거 묶음 생성기**를 함께 만든다.

## 모델

| id | 파일 | 크기 | SHA-256 | 라이선스 |
|---|---|---|---|---|
| `qwen3-4b-q4km` | `Qwen3-4B-Q4_K_M.gguf` | 2,497,280,256 | `7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5` | Apache-2.0 |

- URL: `https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf`. 접근 승인이 필요 없는 저장소.
- 고른 이유: 상업 배포 가능(Apache-2.0), 한국어 가능, 4B Q4로 내장 그래픽·CPU에서 감당 가능(5625U 점검에서 1.5B 기준 "충분"). Qwen2.5-3B는 비상업 라이선스라 제외했다.
- `diag::MODELS`에 추가하고 기존 내려받기·검증·파일 가져오기를 재사용. 「기기 점검」 탭에 「평어 초안 모델」 칸(음성 자막 칸과 같은 방식).
- 나중에 미세조정한 모델로 바꿀 수 있게 settings `evalModelId`로 고른다.

## 실행 방식: llama-server

- llama.cpp 윈도우 빌드(이미 b11600 Vulkan을 번들)에서 `llama-server.exe`도 설치 파일에 넣는다(`scripts/fetch-engines.mjs`가 `main` 하나만 남기던 것을 `keep` 목록으로 바꿈: llama는 `llama-bench.exe`, `llama-server.exe`).
- 처음 초안을 만들 때 `127.0.0.1`의 빈 포트로 띄우고(`--host 127.0.0.1 --port <포트> -m <모델> -c 8192 -ngl 99 --jinja`), `/health`가 준비될 때까지 기다린다. 마지막 요청 뒤 5분 동안 쓰지 않으면 끈다. 앱이 끝날 때도 끈다. **바깥 주소로는 열지 않는다**(127.0.0.1 고정).
- GPU에서 실패하면(시작 실패·충돌) `-ngl 0`으로 한 번 다시 띄운다.
- Qwen3의 생각(thinking) 출력은 끈다. llama.cpp b11600의 `tools/server` 소스에서 지원하는 방법(요청의 `chat_template_kwargs: { enable_thinking: false }` 또는 서버 옵션)을 **소스에서 확인해** 쓴다. 출력에 `<think>`가 섞이면 실패로 처리.
- 요청: `/v1/chat/completions`, `temperature 0.4`, `seed` = 학생 id에서 만든 수(같은 입력이면 같은 초안), `max_tokens 700`, `response_format`에 **JSON 스키마**(llama.cpp가 문법 제약으로 강제):

```json
{
  "type": "object",
  "required": ["sentences"],
  "additionalProperties": false,
  "properties": {
    "sentences": {
      "type": "array", "minItems": 2, "maxItems": 4,
      "items": {
        "type": "object", "required": ["text", "evidence"], "additionalProperties": false,
        "properties": {
          "text": { "type": "string", "minLength": 15, "maxLength": 140 },
          "evidence": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "enum": ["<이 묶음의 근거 id들>"] } }
        }
      }
    }
  }
}
```

  `evidence`의 `enum`을 묶음마다 실제 id로 채우므로 **없는 근거 id는 문법상 나올 수 없다**.
- 결과는 설계 08의 자동 검사를 그대로 거친다. 파싱 실패·`<think>`·검사에서 "근거 없음"이 나오면 한 번 다시(seed+1) 시도하고, 그래도 안 되면 템플릿 초안으로 대신하고 화면에 "언어 모델 초안을 만들지 못해 템플릿 초안을 보여 줍니다"를 띄운다.

## 프롬프트

`data/evaluation/prompt.json`(스키마와 검증 포함)에 둔다. 코드에 한국어 프롬프트를 박지 않는다.

- `system`: 역할(초등 1~2학년 통합교과 담임의 평어 초안 보조), 원칙(근거에 있는 사실만, 문장마다 근거 id, 이름 대신 "학생"도 쓰지 말고 주어 생략, 서열·비교·점수 금지, 가족·사교육·종교·진단명 금지, 다른 아이 언급은 "친구"), 문체(설정값 `evalStyle`에 따라 "~함"/"~음"/"~하는 모습을 보임"), 분량(설정 `evalTargetChars` ±20%), 교과의 성격(바: 실천·습관·관계, 슬: 탐구·질문·발견, 즐: 놀이·표현·협력)과 그 교과 성취기준 원문.
- `examples`: 근거 묶음 → 출력 쌍 2~3개(예시 묶음은 합성). 사용자의 교사 예시 평어를 받으면 이 자리를 바꾼다.
- `user`: 이번 근거 묶음 JSON(설계 08 형식 그대로, 이름 없음).

## 화면

- 「평어」 탭 학생 초안 영역에 생성기 선택: 「템플릿」 / 「언어 모델」(모델이 준비됐을 때만). 기본은 모델이 있으면 언어 모델.
- 언어 모델 생성은 시간이 걸리므로 진행 표시("초안 만드는 중… 학생 1명에 30초~1분")와 「취소」. 학급 일괄 생성(아직 초안이 없는 학생만, 하나씩 순서대로, 진행 막대, 실패 목록).
- 저장되는 초안의 `generator`에 `template` / `llm:qwen3-4b-q4km` 기록.

## 합성 근거 묶음 생성기 (미세조정 준비)

실제 학생 기록 없이 학습 데이터를 만들기 위한 도구. 앱에 들어가지 않는다.

- `data/synthetic/phrases.json`(스키마 `data/synthetic/schema/phrases.schema.json`, 검증 `validate:data`에 포함): 교과·태그 묶음별 **교사 관찰 메모 문장**(1~2학년 교실에서 흔한 행동 관찰, 묶음마다 15개 이상), **아이 말 인용**(짧은 구어, 묶음마다 8개 이상), 주제책별 차시 제목(주제책마다 6개 이상). Haiku drafter가 초안, 메인 세션이 검토.
- `ml/synth/generate-bundles.mjs`: 시드를 받아 가짜 학생 N명 × 교과 3개의 근거 묶음(JSONL)을 만든다. 설계 08 형식과 같고, 근거 개수(3~12)·주제책 분포·자막 유무·습관 실천율을 고르게 섞는다. 같은 시드면 같은 결과. 출력은 `ml/out/`(저장소에 넣지 않음).
- `ml/README.md`: 다음 단계(목표 출력 만들기 → 검수 → LoRA 학습 → GGUF 변환 → `evalModelId`로 교체)와 각 단계의 입력·출력 형식.
- Node 테스트: 생성기가 같은 시드에서 같은 결과, 모든 묶음이 설계 08 형식(근거 id 고유, 3~12개, 이름 없음).

## 테스트 기준

Rust:
- llama-server 실행 부분은 엔진 호출을 주입받아 가짜 서버로 테스트: 정상 JSON → 문장 저장, `<think>` 섞임 → 재시도 후 템플릿 대체, 파싱 실패 → 재시도, 두 번 실패 → 템플릿 대체와 안내.
- 요청 본문: JSON 스키마의 `enum`이 묶음의 근거 id와 정확히 같음, seed가 학생 id에서 결정됨, 시스템 프롬프트에 문체·분량·성취기준이 들어감, **학생 이름이 요청 본문 어디에도 없음**.
- 서버 관리: 127.0.0.1에만 바인딩, 유휴 종료 타이머, 앱 종료 시 종료(프로세스 핸들을 놓치지 않는지).
- `prompt.json` 스키마 검증.

Node: 합성 생성기 재현성·형식.
