# ml/ — 평어 모델 학습 준비

설계 08(학습 데이터 계획)·09(합성 근거 묶음)의 학습 파이프라인 작업 폴더다. 앱에는 들어가지 않는다.

> **경고: 실제 학생 데이터는 절대 이 폴더(`ml/`)에 넣지 않는다.**
> 실제 이름, 관찰 기록, 음성 자막, 앱 DB(`*.sqlite`), 앱에서 내보낸 평어를 여기에 복사하거나 외부 클라우드 모델·학습 서비스로 보내면 안 된다.
> 여기서 다루는 것은 합성(가짜) 근거 묶음과 교사가 이름 없이 직접 쓴 예시뿐이다. `ml/out/`은 `.gitignore`에 있어 저장소에 올라가지 않는다.

## 단계 한눈에 보기

| 단계 | 하는 일 | 입력 | 출력 | 상태 |
|---|---|---|---|---|
| A | 교사 예시 평어 | 교사가 직접 씀 | `ml/data/teacher-examples.jsonl` | 사용자 |
| B | 합성 근거 묶음 | `data/synthetic/phrases.json` | `ml/out/bundles.jsonl` | 구현됨 |
| C | 목표 출력 작성 | `bundles.jsonl` + A | `ml/out/targets.jsonl` | 다음 |
| D | 교사 검수 | `targets.jsonl` | `ml/out/reviewed.jsonl` | 다음 |
| 분할 | 학습/평가 나누기 | `reviewed.jsonl` | `ml/out/train.jsonl`, `ml/out/eval.jsonl` | 다음 |
| E | LoRA 학습, GGUF 변환 | `train.jsonl` | `ml/out/model/*.gguf` | 다음 |
| F | 평가 후 앱에 연결 | `eval.jsonl`, GGUF | settings `evalModelId` | 다음 |

모든 `.jsonl`은 한 줄에 JSON 하나, UTF-8, 줄바꿈 `\n`.

## B. 합성 근거 묶음 만들기 (구현됨)

```bash
node ml/synth/generate-bundles.mjs --students 200 --seed 1 --phrases data/synthetic/phrases.json --out ml/out/bundles.jsonl
node --test ml/synth/generate-bundles.test.mjs
```

- 같은 `--seed`면 바이트까지 같은 결과. 학생 수 × 교과 3개(바·슬·즐)의 묶음이 나온다. 1,000~3,000개가 목표면 `--students 400` 안팎.
- 한 줄 = 묶음 하나(근거 3~12개, 설계 08 형식 그대로):

```json
{"meta":{"syntheticStudent":"synth-0001","subject":"바","seed":1},"bundle":{"student":{"alias":"학생"},"subject":"바","period":{"from":"2027-03-02","to":"2027-07-18"},"evidence":[{"id":"h:…","kind":"habit","title":"…","schoolDays":33,"doneDays":25},{"id":"o:…","kind":"observation","date":"2027-03-09","topic":"나","standards":["[2바01-02]"],"tags":["배려"],"text":"…","quote":"…","together":false},{"id":"s:…","kind":"session","date":"2027-03-23","topic":"나","title":"…","standards":["[2바01-02]"]}],"standardsText":{"[2바01-02]":"…"}}}
```

- 문장 재료는 `data/synthetic/phrases.json`(검증: `node scripts/validate-synthetic.mjs`). 이름·금지 표현·묶음 안 중복을 검사한다.

## A. 문체 예시

### A-1. AI가 만든 문체 예시 (있음)

`ml/data/style-examples.jsonl` — 교사가 예시를 주기 전까지 쓰는 출발점. 72개(교과별 48 = 주제책 16권 × 바·슬·즐, 세 교과 통합본 24 = 학년·학기마다 6개). 모두 지어낸 활동이며 실제 학생 기록이 아니다(`"source": "ai-draft"`).

- 근거: 「학교생활기록 작성 및 관리지침」(교육부훈령, 2026.3.1. 시행) 제15조 제2항 — 1·2학년 바른생활·슬기로운 생활·즐거운 생활은 「성취수준 및 특기사항」에 **통합하여 입력**. 기재요령 — 세 교과가 누락되지 않게 종합하고, 명사형 어미로 종결하며, 특수문자·번호 기호는 지양.
- 한 줄 예: `{"id":"int-01","source":"ai-draft","kind":"integrated","grade":1,"semester":1,"topics":["학교","사람들"],"standards":["[2바01-01]",…],"style":"ham","profile":"고르게 참여","text":"…함. …함."}`
  - `kind`: `subject`(한 교과 1~2문장, 40~200자) / `integrated`(세 교과 통합, 200~500자). `profile`은 학습용 설명이고 평어에 들어가지 않는다.
- 검증: `node scripts/validate-style-examples.mjs` (`npm run validate:data`에 포함). 금지 표현, 「학생」·이름 자리표시, 특수문자·번호, 문장마다 명사형 종결, 문체(`ham`/`eum`/`boim`) 일관, 통합본의 세 교과 성취기준, 주제책과 학년·학기 대응을 검사한다.
- **교사 검수 필요**: 학교 문체와 다르면 고치거나 지우고, 교사가 쓴 예시는 `"source": "teacher"`로 같은 파일에 더한다.

### A-2. 교사 예시 (사용자)

`ml/data/teacher-examples.jsonl` — 실명·실제 학생 정보를 지운 평어 50~100개. 받으면 A-1과 같은 형식(`source: "teacher"`)으로 옮겨 검증한다.

## C. 목표 출력 만들기

B의 묶음마다 A의 문체와 설계 08 원칙(근거에 있는 사실만, 문장마다 근거 id, 서열·비교 금지, 이름 금지)으로 평어를 쓴다. 큰 클라우드 모델을 쓰더라도 **보내는 것은 합성 묶음과 교사 예시뿐**이다.
`ml/out/targets.jsonl`:

```json
{"meta":{"syntheticStudent":"synth-0001","subject":"바","seed":1},"style":"ham","bundle":{…},"output":{"sentences":[{"text":"…함.","evidence":["o:…","h:…"]}]}}
```

만든 직후 자동 검사(앱의 `check_draft`와 같은 기준)를 돌려 근거 id가 묶음에 없는 줄, 금지 표현, 문장 수(2~4)·길이 범위 밖인 줄은 버린다.

## D. 검수

`targets.jsonl`에서 200~300줄을 골라 교사가 읽고 고친다. 결과 `ml/out/reviewed.jsonl`은 `targets.jsonl`과 같은 형식에 두 칸을 더한다.

```json
{ …, "review": {"status":"accepted"|"edited"|"rejected","note":""} }
```

`rejected`는 학습에서 뺀다. 평가용 100개는 이 단계에서 따로 표시해 둔다(학습에 섞지 않음).

## 분할

- `ml/out/eval.jsonl`: 평가용 100줄. `meta.syntheticStudent` 기준으로 나눠 같은 가짜 학생이 학습과 평가에 같이 들어가지 않게 한다.
- `ml/out/train.jsonl`: 나머지. 대화 형식으로 바꾼다(프롬프트는 `data/evaluation/prompt.json`의 system과 같게).

```json
{"messages":[{"role":"system","content":"…"},{"role":"user","content":"<bundle JSON>"},{"role":"assistant","content":"{\"sentences\":[…]}"}]}
```

## E. 학습과 변환

- 기반 모델: 상업 배포 가능한 라이선스의 3~4B급 한국어 공개 모델(현재 연결된 `qwen3-4b-q4km`, Apache-2.0. 학습 시점에 라이선스를 다시 확인).
- LoRA 학습: 빌린 GPU에서 `train.jsonl`로 실행. 출력은 어댑터(`ml/out/model/adapter/`). 학습 과정에도 실제 학생 데이터를 올리지 않는다.
- 병합 후 GGUF Q4_K_M 변환(llama.cpp의 변환·양자화 도구): `ml/out/model/<이름>-Q4_K_M.gguf`. SHA-256과 크기를 기록해 둔다.

## F. 평가와 교체

`eval.jsonl`의 100개 묶음에 모델을 돌려 확인한다.

- 근거 연결 100%(없는 id 0), 금지 표현 0, 길이 범위 안.
- 교사 평가(자연스러움·정확성) 점수를 템플릿 초안과 같은 묶음으로 비교.

통과하면 `diag::MODELS`에 새 모델(id, 파일명, 크기, SHA-256, 라이선스)을 추가하고, 설정 `evalModelId`를 그 id로 바꾼다. 저장되는 초안의 `generator`는 `llm:<id>`로 남는다.

## 알려진 한계 (메인 세션 검토)

- 관찰 메모와 아이 말 인용을 서로 따로 뽑기 때문에, 인용이 메모 내용과 어울리지 않는 경우가 있다(예: 물감 정리 메모 + "손 씻고 와도 돼요?"). 목표 출력(C단계)을 만들기 전에 같은 묶음 안에서 메모와 인용의 주제를 맞추도록 문장 은행에 짝 정보를 더하거나, 인용은 같은 묶음 키(group)의 것만 쓰도록 생성기를 좁혀야 한다.
