# 바슬즐 탐험대 (가칭)

2022 개정 초등 통합교과(바른 생활·슬기로운 생활·즐거운 생활) 1~2학년군 수업을 돕는 윈도우 데스크톱 앱입니다.
교사 PC 한 대에서 서버 없이 동작하고, AI도 기기 안의 로컬 모델로 처리하는 것을 목표로 합니다.

개발 계획은 [docs/PLAN.md](docs/PLAN.md)에 있습니다.

## 현재 상태 (v0.1.0)

- 교육과정 데이터: 영역 4개, 주제책 16권, 성취기준 48개(교육부 고시 원문, 해설·적용 시 고려 사항 포함), 단원 개발 도구 8종, 교과별 교수학습 모형
- 앱 화면: 학기별 주제책과 대응 성취기준 보기, 단원 개발 도구 보기
- CI: 데이터 검증, 타입 검사, 프론트엔드 빌드, 윈도우 설치 파일(서명 전) 빌드

## 개발 환경

필요한 것: Node.js 22 이상, Rust(stable), 윈도우에서는 [Tauri 사전 요구사항](https://v2.tauri.app/start/prerequisites/)(Microsoft C++ Build Tools, WebView2)

```bash
npm install
```

데스크톱 앱 개발 모드로 실행:

```bash
npm run tauri dev
```

브라우저에서 화면만 확인:

```bash
npm run dev
```

교육과정 데이터 검증:

```bash
npm run validate:data
```

윈도우 설치 파일 빌드(윈도우에서 실행):

```bash
npm run tauri build -- --bundles nsis
```

빌드된 설치 파일은 `src-tauri/target/release/bundle/nsis/`에 생깁니다.
GitHub Actions에서도 `main`에 푸시할 때마다 빌드되며, 실행 결과의 Artifacts에서 내려받을 수 있습니다.

## 폴더 구조

```
data/curriculum/            교육과정 데이터(JSON)와 스키마
scripts/                    데이터 검증 스크립트
src/                        화면(React + TypeScript)
src-tauri/                  데스크톱 앱(Rust, Tauri 2)과 설치 파일 설정
docs/                       개발 계획 문서
```

## 교육과정 데이터 다루기

`data/curriculum/integrated-2022.json`이 앱 전체에서 쓰는 기준 데이터입니다.

- 주제책은 `area`(영역)와 `standardIndex`(영역 안 번호)로 성취기준에 연결됩니다. 예를 들어 「학교」는 1영역 1번이므로 `[2바01-01]`, `[2슬01-01]`, `[2즐01-01]`에 대응합니다.
- `verification` 값은 출처 상태를 나타냅니다.
  - `unverified-spec`: 명세서에서 옮겼고 아직 확인하지 않음
  - `inferred`: 추정
  - `verified`: 고시 원문이나 교사용 지도서로 확인함
- 성취기준 원문은 국가교육과정정보센터(NCIC) 고시문으로 확인한 뒤 `statement`에 넣고 `verification`을 `verified`로 바꿔 주세요.
- 교과는 코드에 고정하지 않고 데이터(`subjects`)로 정의합니다. 교과가 추가되면 데이터만 늘리면 됩니다.
- 수정한 뒤에는 `npm run validate:data`로 코드 형식, 중복, 주제책과 성취기준 대응 관계를 검사하세요.
