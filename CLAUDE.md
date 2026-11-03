# 작업 규칙

## 역할 분담 (사용자 결정)

메인 세션(Opus)이 설계·검토·커밋을 맡고, 실제 작업은 `.claude/agents/`의 워커에게 맡긴다.

| 워커 | 모델 | 맡는 일 |
|---|---|---|
| `implementer` | Sonnet | 설계 문서를 받아 기능 구현, 단위 테스트, 브라우저 스모크 테스트 |
| `drafter` | Haiku | 스키마에 맞춘 데이터·콘텐츠 초안(놀이 카드, 활동 카드 등) |
| `verifier` | Haiku | 파일을 고치지 않고 확인 명령·스모크 테스트·CI 결과 확인 후 보고 |

작업 흐름:

1. 메인 세션: 설계 문서(`docs/design/NN-*.md`)와 필요한 스키마를 쓰고 커밋한다.
2. 서로 겹치지 않는 일은 워커를 동시에 돌린다(예: implementer는 코드, drafter는 데이터 파일). 맡길 파일을 분명히 나눠 준다.
3. 워커는 커밋·푸시하지 않는다.
4. 메인 세션: verifier로 확인 명령을 독립 실행하고, 위험한 코드(데이터 삭제·이동, 마이그레이션, 트랜잭션)는 직접 읽어 검토한다. 콘텐츠 초안은 직접 검토한다.
5. 메인 세션: 테스트가 실패하면 커밋하지 않는다(명령을 `&&`로 잇지 말고 결과를 확인한 뒤 커밋). 커밋·푸시 후 verifier로 윈도우 CI 결과를 확인한다.

에이전트 정의가 이 세션에서 아직 안 보이면 `Agent` 도구에 `subagent_type: "general-purpose"`, `model`을 지정하고 해당 정의 파일을 읽으라고 지시한다.
단순 수정(오타, 설정 한두 줄)은 메인 세션이 직접 해도 된다.

## 프로젝트 개요

2022 개정 초등 통합교과 1~2학년 수업 지원 윈도우 앱(Tauri 2 + React + TypeScript, Rust 백엔드).
서버 없이 교사 PC 한 대에서 동작하고, AI는 로컬 모델(llama.cpp, whisper.cpp)로 처리한다.
계획: `docs/PLAN.md`. 설계: `docs/design/`.

## 원칙

- 아이 데이터는 교사 PC 밖으로 보내지 않는다. 네트워크 요청은 기기 점검의 모델 내려받기 외에는 추가하지 않는다.
- 화면 문구는 한국어. 눌러야 하는 요소의 터치 영역은 최소 64px.
- 교육과정 데이터(`data/curriculum/integrated-2022.json`)는 출처 상태(`verification`)를 유지한다. 원문 확인 없이 `verified`로 바꾸지 않는다.

## 확인 명령

```bash
npm run validate:data
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

리눅스에서 `cargo` 명령은 `dist/`가 있어야 컴파일되므로 `npm run build`를 먼저 실행한다.
윈도우 설치 파일은 GitHub Actions(`.github/workflows/ci.yml`)에서 빌드된다.
