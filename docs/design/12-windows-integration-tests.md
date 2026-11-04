# 설계 12: 윈도우 통합 테스트 (엔진 실행, 화면 E2E)

지금까지 "실제 윈도우·WebView2에서 확인하지 못함"으로 남은 부분을 GitHub Actions 윈도우 러너에서 자동으로 확인한다. 평소 커밋마다 도는 `ci.yml`과 분리한 워크플로 두 개로 둔다(모델 내려받기와 앱 실행에 시간이 걸리므로). 실행: 수동(`workflow_dispatch`)과 주 1회(`schedule`), 그리고 `main`에 `v*` 태그를 붙일 때.

## 1. 엔진 통합 테스트 — `.github/workflows/engines.yml`

목적: 앱이 쓰는 **실제 인자와 요청 본문**으로 번들 엔진이 동작하는지 확인한다.

- 준비: `npm ci`, `npm run fetch:engines`, `npm run build`(Rust가 `dist/`를 요구), 테스트 모델 내려받기(`actions/cache`로 보관, SHA-256 확인):

| 용도 | 파일 | 크기 | SHA-256 |
|---|---|---|---|
| 음성 인식 | `ggml-tiny-q5_1.bin` (ggerganov/whisper.cpp) | 32,152,673 | `818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7` |
| 언어 모델 | `Qwen3-0.6B-Q8_0.gguf` (Qwen/Qwen3-0.6B-GGUF, Apache-2.0) | 639,446,688 | `9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031` |

- 테스트: `src-tauri/tests/engines.rs`(통합 테스트, 기본은 `#[ignore]`, 워크플로에서 `cargo test --test engines -- --ignored --nocapture`로 실행). 엔진·모델 경로는 환경 변수 `TONGHAP_ENGINES_DIR`, `TONGHAP_TEST_WHISPER_MODEL`, `TONGHAP_TEST_LLM_MODEL`로 받는다. 앱 코드의 **같은 함수**를 불러야 한다(인자 목록을 테스트에 복사하지 않는다 — 필요하면 앱 코드에서 인자 만드는 함수를 `pub`으로 연다).
  1. `whisper-cli`: 앱의 자막 실행 인자(`-l ko` 포함) 그대로 `resources/samples/jfk.wav`를 받아쓰기 → 성공 종료, `clean_transcript` 결과가 비어 있지 않음(영어 음성을 `ko`로 돌리면 번역·음역이 나올 수 있으므로 내용은 검사하지 않음). 같은 파일을 `-l en`으로 한 번 더 돌려 "fellow Americans"가 들어 있는지 확인(엔진 자체 동작 확인).
  2. 앱 화면이 만드는 것과 같은 16kHz 모노 WAV: Rust 쪽 `validate_wav`를 통과하는 WAV를 테스트에서 만들고(1초 무음) 받아쓰기 → "말소리를 찾지 못했습니다"로 처리되는지(빈 결과 처리 경로).
  3. `llama-bench`: 앱의 GPU·CPU 측정 인자로 실행 → `parse_llama_bench` 성공.
  4. `llama-server`: 앱의 서버 관리 코드(`llm_server`)로 실제 시작 → `/health` 준비 → 합성 근거 묶음(설계 08 형식, 근거 5개)으로 앱의 요청 본문을 그대로 만들어 전송(JSON 스키마의 근거 id `enum`, `enable_thinking:false`) → 응답이 JSON으로 파싱되고, 모든 `evidence`가 묶음 id 안에 있고, `<think>`가 없음. 0.6B 모델이라 문장 품질은 검사하지 않는다. 끝나면 서버 종료와 프로세스 정리 확인.
  5. 작업 개체(KILL_ON_JOB_CLOSE)로 붙인 `llama-server`가 테스트용 작업 개체를 닫으면 끝나는지(이미 단위 테스트가 있으면 생략).
- 실패하면 엔진 stderr 끝부분을 출력한다.

## 2. 화면 E2E — `.github/workflows/e2e.yml`

목적: 실제 WebView2에서 앱이 뜨고 주요 흐름이 동작하는지(권한, CSP, IPC, asset 프로토콜, 창 만들기).

- 도구: `tauri-driver`(cargo install, 버전 고정) + 러너의 Edge 버전에 맞는 `msedgedriver`(WebView2와 같은 버전대; 맞추는 방법은 tauri 문서의 윈도우 WebDriver 안내를 따른다) + Node 테스트 러너에서 `selenium-webdriver` 또는 `webdriverio`(devDependency, 버전 고정 — 하나만).
- 앱은 `cargo build`(디버그 아님 — `tauri build --debug --no-bundle` 등 tauri 문서가 권하는 방식)로 만든 실행 파일을 직접 띄운다. **테스트용 데이터 폴더**를 쓰도록 환경 변수 `TONGHAP_DATA_DIR`이 있으면 `app_local_data_dir()` 대신 그 경로를 쓰게 앱에 작은 분기를 넣는다(릴리스에서도 무해; 없으면 기존 동작).
- 시나리오(`e2e/app.test.mjs`):
  1. 앱이 뜨고 「관찰 기록」 탭이 보임, 콘솔 오류 없음(가능하면 브라우저 로그 수집).
  2. 학급 만들기 → 명단 3명 붙여넣기 → 관찰 기록 저장(학생 1명, 태그 1개, 메모) → 「기록 찾기」에서 메모 낱말로 검색되어 나옴.
  3. 포트폴리오 → 학생 → 타임라인에 기록.
  4. 단원 → 새 단원(주제책 + 도구) → 활동 카드에서 차시 추가.
  5. 「칠판 모드」 버튼 → 두 번째 창(`board`)이 생김(창 핸들 수로 확인) → 칠판 창에서 타이머 탭이 보임 → 창 닫기.
  6. 「정보」 탭 → 버전이 `package.json`과 같음, 데이터 폴더 경로가 `TONGHAP_DATA_DIR` 아래.
  7. 「기기 점검」 → 시스템 정보 표가 채워짐(Vulkan은 러너에 없을 수 있으므로 "사용 불가"도 통과).
- 사진·음성·저장 대화상자처럼 네이티브 대화상자가 필요한 흐름은 이번 범위에서 뺀다.
- 실패하면 스크린숏을 아티팩트로 올린다.

## 완료 기준

- 두 워크플로가 `workflow_dispatch`로 한 번씩 성공(메인 세션이 실행해 확인).
- 기존 `ci.yml`과 로컬 확인 명령은 그대로 통과(엔진 테스트는 `#[ignore]`라 평소 `cargo test`에 영향 없음).
