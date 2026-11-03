# 작업 규칙

## 역할 분담 (사용자 결정)

- **메인 세션(Opus)**: 설계, 설계 문서(`docs/design/`) 작성, 구현 결과 검토·테스트, 커밋·푸시.
- **구현**: Sonnet 서브에이전트(Agent 도구, `model: "sonnet"`)에게 맡긴다. 설계 문서 경로와 완료 기준을 주고, 커밋·푸시는 하지 않게 한다.
- 단순 수정(오타, 설정 한두 줄)은 메인 세션이 직접 해도 된다.

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
