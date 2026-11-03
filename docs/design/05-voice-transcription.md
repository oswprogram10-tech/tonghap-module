# 설계 05: 음성 메모 자동 자막

관찰 기록의 음성 메모를 **교사 PC 안에서** whisper.cpp로 받아써서 첨부의 `transcript`에 넣는다.
자막은 포트폴리오에서 읽고 검색할 수 있게 하고, 나중에 평어 초안의 근거로 쓴다. 아이 음성 인식은 틀리기 쉬우므로 **교사가 고칠 수 있어야** 한다.

## 범위

포함: 자막 모델 준비(내려받기·파일 선택), 녹음 저장 뒤 자동 받아쓰기(대기열, 한 번에 하나), 수동 「자막 만들기」·「다시 만들기」, 자막 수정, 진행 상태 표시, 기록 화면·포트폴리오에 자막 표시.
제외: 아동 음성 미세조정 모델(이후), 실시간 받아쓰기, 화자 구분, 검색 기능(다음 설계에서 관찰 검색과 함께).

## 엔진과 모델

- 엔진: 이미 번들된 `whisper-cli`(whisper.cpp v1.9.2 윈도우 CPU 빌드, `resources/engines/whisper/`). 새 엔진을 추가하지 않는다.
- 모델 목록은 `src-tauri/src/diag/mod.rs`의 `MODELS`에 추가하고, 내려받기·검증·파일 가져오기는 기존 `diag_download_model`, `diag_import_model`을 그대로 쓴다.

| id | 파일 | 크기 | SHA-256 | 용도 |
|---|---|---|---|---|
| `whisper-small-q5` | `ggml-small-q5_1.bin` | 190,085,487 | `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb` | **기본** (한국어 정확도와 속도 균형) |
| `whisper-base-q5` | `ggml-base-q5_1.bin` | 59,707,625 | `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898` | 느린 PC용 |

  URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/<파일>`. 라이선스 MIT. 기존 `whisper-base`(기기 점검용)는 그대로 둔다.
- 사용할 모델: settings `sttModelId`(기본 `whisper-small-q5`). 그 모델 파일이 없으면 자막 기능은 "모델 필요" 상태.

## 오디오 변환 (프런트엔드)

whisper-cli는 WAV·MP3·FLAC만 읽는다(miniaudio). 녹음은 webm/opus이므로 **화면에서 16kHz 모노 PCM16 WAV로 바꿔** 넘긴다.

1. 첨부 파일을 `convertFileSrc(absPath)`로 `fetch` → `ArrayBuffer`.
2. `AudioContext.decodeAudioData`로 디코드(WebView2는 webm/opus·ogg·m4a·wav를 디코드할 수 있다).
3. `OfflineAudioContext(1, ceil(duration*16000), 16000)`로 모노 16kHz 리샘플.
4. Float32 → PCM16 WAV 인코딩(`src/stt/wav.ts`의 순수 함수 `encodeWav16(samples: Float32Array, sampleRate): Uint8Array`).
5. 5분 넘는 녹음은 받아쓰지 않고 "녹음이 너무 깁니다(5분 이하)".

## 대기열과 상태 (Rust)

- `attachments`에 자막 상태 칼럼 추가(마이그레이션 4):

```sql
ALTER TABLE attachments ADD COLUMN transcript_status TEXT NOT NULL DEFAULT 'none'
  CHECK (transcript_status IN ('none', 'queued', 'running', 'done', 'failed'));
ALTER TABLE attachments ADD COLUMN transcript_error TEXT;
ALTER TABLE attachments ADD COLUMN transcript_model TEXT;
ALTER TABLE attachments ADD COLUMN transcript_edited INTEGER NOT NULL DEFAULT 0;
ALTER TABLE attachments ADD COLUMN transcribed_at TEXT;
```

- `stt_transcribe` 명령: 원시 바이트 본문(WAV) + 헤더 `x-attachment-id`. 검사: 음성 첨부인지, WAV 헤더가 RIFF/WAVE·PCM16·모노·16000Hz인지, 길이 ≤ 5분. 통과하면 `data/stt-tmp/<attachmentId>.wav`에 쓰고 상태 `queued`로 바꾼 뒤 즉시 돌려준다.
- 작업자: 앱 시작 때 스레드 하나를 띄워 대기열(채널)을 순서대로 처리한다. 한 번에 whisper-cli 하나만 돈다.
  - 상태 `running` → whisper-cli 실행 → 결과 정리 → `transcript`, `transcript_status='done'`, `transcript_model`, `transcribed_at`, `transcript_edited=0` 저장 → 임시 WAV 삭제.
  - 실패하면 `failed` + `transcript_error`(한국어 한 줄 + stderr 끝부분 일부). 임시 WAV 삭제.
  - 상태가 바뀔 때마다 이벤트 `stt://updated` `{ attachmentId, status }`를 보낸다.
  - 앱 시작 때 `queued`·`running`으로 남아 있는 첨부는 `none`으로 되돌린다(임시 WAV가 사라졌으므로). 화면은 `none`이고 자동 받아쓰기가 켜져 있으면 다시 요청할 수 있다.
- whisper-cli 실행은 `diag::bench`의 `run`(콘솔 창 없이, 시간 제한)을 공유 모듈로 옮겨 재사용한다. 인자:
  `-m <모델> -f <wav> -l ko -t <min(물리코어,8)> -nt -np` 시간 제한 10분.
  - `--prompt`는 쓰지 않는다. whisper-cli(v1.9.2)는 윈도우에서 명령줄 인자를 시스템 코드 페이지(한국어 윈도우는 CP949)로 받지만 프롬프트는 UTF-8로 토큰화하므로 한국어 프롬프트가 깨진다. 파일 경로는 같은 코드 페이지로 열기 때문에 한글 경로도 동작한다.
- 결과 정리(순수 함수 `clean_transcript`): 줄 앞뒤 공백 제거, `[BLANK_AUDIO]`·`[음악]`·`(웃음)`처럼 대괄호·소괄호만 있는 줄 제거, 연속 공백 하나로, 빈 결과면 `done`이 아니라 `failed`("말소리를 찾지 못했습니다").
- 자막 실행 함수는 엔진 호출을 주입받게 해서(`&dyn Fn(&Path, &Path) -> Result<String, String>`) 테스트에서 가짜 엔진을 쓴다.

## Tauri 명령

| 명령 | 입력 | 출력 |
|---|---|---|
| `stt_status` | — | `{ modelId, modelReady, engineReady, auto, queueLength }` |
| `stt_set_model` | `{ modelId }` | — (`whisper-small-q5` 또는 `whisper-base-q5`만) |
| `stt_set_auto` | `{ auto }` | — (settings `sttAuto`, 기본 true) |
| `stt_transcribe` | 원시 WAV 본문 + `x-attachment-id` | `{ status: 'queued' }` |
| `attachment_set_transcript` | `{ id, text }` | `Attachment` (`transcript_edited=1`, 상태 `done`) |

- `Attachment` 응답에 `transcriptStatus`, `transcriptError`, `transcriptEdited`, `transcriptModel`을 추가한다.

## 화면

- **기록 화면 음성 메모**(최근 기록 카드·수정 창·포트폴리오 타임라인 공통 컴포넌트):
  - 재생기 아래에 자막 영역. 상태별: `none` → 「자막 만들기」 / `queued` → "자막 대기 중" / `running` → "자막 만드는 중…"(점 애니메이션) / `done` → 자막 글(교사가 고쳤으면 "고침" 배지) + 「고치기」, 「다시 만들기」 / `failed` → 오류 문구 + 「다시 시도」.
  - 「고치기」: 여러 줄 입력 → 저장(`attachment_set_transcript`).
  - 「다시 만들기」는 고친 자막을 덮어쓰므로 확인을 받는다.
- **자동**: 관찰 기록을 저장하며 음성 첨부를 올린 직후, `auto`이고 모델이 준비돼 있으면 화면이 WAV로 바꿔 `stt_transcribe`를 부른다. 모델이 없으면 기록 화면 위쪽에 한 번 안내("음성 자막 모델이 없습니다 — 기기 점검 탭에서 내려받기") 배너를 띄운다(「다시 보지 않기」).
- **모델 설정**: 「기기 점검」 탭에 「음성 자막」 칸 추가 — 사용할 모델 선택(기본·느린 PC용), 모델 상태(기존 모델 행 컴포넌트 재사용: 내려받기·파일 선택·진행 막대), 자동 받아쓰기 켜기/끄기.
- `stt://updated` 이벤트를 받으면 해당 첨부만 다시 불러온다(목록 전체 새로 고침 대신 `observation_list` 재호출도 허용).

## 테스트 기준

Rust:
- 마이그레이션 4(버전 3 데이터 유지, 새 칼럼 기본값), 버전 3 백업 복원 시 자동 마이그레이션.
- WAV 헤더 검사: 올바른 16kHz 모노 PCM16 통과, 44.1kHz·스테레오·8비트·RIFF 아님·5분 초과 거부.
- `clean_transcript`: 대괄호·소괄호 전용 줄 제거, 공백 정리, 빈 결과는 오류.
- 대기열 처리(가짜 엔진): 성공 시 `done`과 자막·모델·시각 저장, 실패 시 `failed`와 오류, 임시 WAV 삭제, 두 작업을 넣으면 순서대로 처리.
- 사진 첨부·없는 첨부에 `stt_transcribe` 거부.
- 앱 시작 시 `queued`·`running` → `none` 복구.
- `attachment_set_transcript`: 수정 표시, 빈 글 허용 여부(빈 글이면 자막 지움, 상태 `none`).

프런트엔드(`node --test`, 기존 `npm test`에 추가):
- `encodeWav16`: 헤더 필드(RIFF 크기, fmt 청크, 샘플레이트, 비트 수, data 크기), 샘플 클리핑(±1 초과), 길이 0.
