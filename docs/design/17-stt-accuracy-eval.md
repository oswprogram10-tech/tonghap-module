# 설계 17: 자막 정확도 측정 도구 (개발자용)

계획(`docs/PLAN.md` 로컬 AI 계획)은 "기본 Whisper의 오류율을 재고, 필요하면 미세조정"이다. 아이 음성 녹음을 모으기 전에 **잴 도구**를 먼저 만들어 둔다. 녹음이 생기면 폴더만 넣고 돌린다. 앱에는 들어가지 않는다.

## 1. 평가 묶음 형식

```
<폴더>/
  manifest.json      {"name": "...", "consent": "보호자 동의 받음(2027-03)", "items": [{"id": "a01", "wav": "a01.wav", "text": "개미가 줄을 서서 가요", "speaker": "child-1", "tags": ["교실소음"]}]}
  a01.wav            16kHz mono 16bit (앱의 녹음과 같은 형식; 다르면 오류로 알림)
```

- `speaker`는 익명 기호만. 이름 금지(검사: 한글 2~4자만으로 된 값이면 경고).
- 스키마: `ml/stt/manifest.schema.json`.
- 저장소에 아이 음성을 넣지 않는다. `ml/stt/`의 `.gitignore`가 `data/`와 `*.wav`를 막고, 예시 묶음 `ml/stt/example/`은 **합성 음성 없이** manifest 형식만 보여 준다(wav 없는 항목은 건너뜀 표시로 테스트).

## 2. 측정 스크립트 `ml/stt/eval.mjs`

```
node ml/stt/eval.mjs --set <폴더> --whisper <whisper-cli 경로> --model <ggml 파일> [--model <다른 파일>] [--threads 4] [--out <결과 폴더>]
```

- 각 항목을 앱과 **같은 인자**로 실행한다. 인자 목록은 `src-tauri/src/stt.rs`에서 읽어 오지 말고, Rust 쪽에 `pub const WHISPER_ARGS`(또는 인자를 만드는 함수)를 두고 그 값을 `ml/stt/whisper-args.json`으로 내보내는 테스트를 둔다(값이 달라지면 Rust 테스트가 실패해 둘이 어긋나지 않게).
- 지표(한국어는 띄어쓰기가 흔들리므로 CER이 주 지표):
  - **CER**: 정규화 후 음절(글자) 단위 편집 거리 / 기준 글자 수.
  - **WER**: 공백 기준 낱말 편집 거리(참고용).
  - 정규화: NFC, 문장부호·공백 제거(CER), 숫자는 그대로, 영어는 소문자.
  - 실행 시간(초)과 음성 길이 대비 배율.
- 결과: `<out>/report-<시각>.md`(모델별 전체 CER·WER·평균 배율, 태그별 CER, 가장 틀린 10개 항목의 기준/결과), 같은 이름의 `.json`.
- 편집 거리·정규화는 `ml/stt/metrics.mjs`로 분리하고 `metrics.test.mjs`(npm test에 추가)로 확인: 같은 문장 0, 한 글자 바꿈, 띄어쓰기만 다를 때 CER 0, 빈 기준 처리, 한글 조합형(NFD) 입력.

## 3. CI

- `engines.yml`(윈도우)에 단계 하나: 엔진·tiny 모델로 작은 묶음 `ml/stt/smoke/`(manifest만 있고 wav는 `src-tauri/resources/`의 영어 `jfk.wav`를 상대 경로로 가리킴, 기준 문장은 jfk 원문)를 돌려 스크립트가 끝까지 돌고 보고서를 만드는지만 확인(CER 상한은 넉넉히 0.5).

## 4. 미세조정 판단 기준 (문서에만)

`ml/stt/README.md`: 녹음 모으는 법(보호자 동의, 이름 말하지 않기, 교실 소음 그대로, 10~30초), 묶음 만드는 법, 실행법, 판단 기준(아이 음성 small CER가 15% 이하면 미세조정 보류, 25% 넘으면 미세조정 검토 — 숫자는 첫 측정 뒤 조정).

## 테스트 기준

- `npm test`에 지표 테스트 추가, 통과.
- Rust: 인자 상수와 `whisper-args.json`이 같은지.
- `engines.yml` 스모크 단계 성공(윈도우).
