# 바슬즐 탐험대 (가칭)

2022 개정 초등 통합교과(바른 생활·슬기로운 생활·즐거운 생활) 1~2학년군 수업을 돕는 윈도우 데스크톱 앱입니다.
교사 PC 한 대에서 서버 없이 동작하고, AI도 기기 안의 로컬 모델로 처리하는 것을 목표로 합니다.

개발 계획은 [docs/PLAN.md](docs/PLAN.md)에 있습니다.

## 현재 상태 (v0.11.1)

- 교육과정 데이터: 영역 4개, 주제책 16권, 성취기준 48개(교육부 고시 원문, 해설·적용 시 고려 사항 포함), 단원 개발 도구 8종, 교과별 교수학습 모형
- 앱 화면: 학기별 주제책과 대응 성취기준 보기
- 관찰 기록: 학급·명단 관리(붙여넣기), 학생·주제책·성취기준·활동 태그·메모·사진·음성 메모로 기록, 수정·휴지통 (설계: [docs/design/01-observation-portfolio.md](docs/design/01-observation-portfolio.md))
- 포트폴리오: 학급 현황(학생 × 주제책 기록 수), 학생별 성취기준 근거 격자와 타임라인
- 정보: 버전, 데이터 위치와 사용량, 마지막 백업, 업데이트 방법, 설치 경고 안내, 오픈소스·모델 라이선스. 인터넷 접속 없음 (설계: [docs/design/11-about-screen.md](docs/design/11-about-screen.md), 선생님용 안내: [docs/INSTALL.md](docs/INSTALL.md))
- 기기 점검: 학교 PC의 사양·Vulkan GPU·인터넷 연결 확인, 측정용 모델 내려받기(이어받기, 파일 직접 선택 가능), llama.cpp·whisper.cpp 속도 측정, 평어 생성·음성 자막 소요 시간 추정, 결과를 문서 폴더에 저장
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

점검 도구용 엔진(llama.cpp·whisper.cpp 윈도우 실행 파일) 내려받기. 윈도우에서 한 번 실행하면 됩니다:

```bash
npm run fetch:engines
```

엔진 버전은 `src-tauri/engines.json`에 고정돼 있습니다.

브라우저에서 화면만 확인:

```bash
npm run dev
```

제3자 라이선스 전문 다시 만들기(의존성을 바꾼 뒤, cargo-about 0.9.2 필요: `cargo install cargo-about --locked --version 0.9.2 --features cli`):

```bash
npm run licenses:full
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

## 기기 점검 사용법

1. 설치 파일로 앱을 설치하고 「기기 점검」 탭을 엽니다.
2. 측정용 모델을 내려받습니다. 언어 모델(빠른 점검 0.5B 또는 정밀 점검 1.5B) 하나와 음성 인식 모델이 필요합니다. 인터넷이 막힌 PC라면 다른 PC에서 받은 파일을 「파일 선택」으로 가져옵니다(SHA-256으로 확인).
   - 모델 파일 주소는 `src-tauri/src/diag/mod.rs`의 `MODELS`에 있습니다.
3. 「측정 시작」을 누릅니다. GPU(Vulkan), CPU, 음성 인식 순서로 측정합니다.
4. 메모(예: 본관 3층 1-2반 교사 PC)를 적고 「결과 저장」을 누르면 `문서\바슬즐 기기점검\`에 JSON과 텍스트 파일이 저장됩니다.

판정은 작은 모델의 측정값을 3B급 모델(약 2GB) 크기에 비례해 환산한 추정치입니다.

## 폴더 구조

```
data/curriculum/            교육과정 데이터(JSON)와 스키마
data/plays/                 놀이 카드(JSON)와 스키마
data/activities/            활동 카드(JSON)와 스키마
data/evaluation/            평어 템플릿·금지 표현(JSON)과 스키마
scripts/                    데이터 검증 스크립트
src/                        화면(React + TypeScript). src/records/ 관찰 기록·포트폴리오, src/units/ 단원 빌더, src/board/ 칠판 모드, src/reports/ 학부모 리포트, src/evaluation/ 평어 초안
src-tauri/                  데스크톱 앱(Rust, Tauri 2)과 설치 파일 설정
ml/                         평어 모델 미세조정 준비(합성 근거 묶음 생성기, 앱에는 들어가지 않음)
src-tauri/src/db/           관찰 기록 저장소(SQLite, 첨부 파일)
src-tauri/src/diag/         기기 점검 기능(사양 조회, 모델 내려받기, 속도 측정)
src-tauri/resources/        설치 파일에 들어가는 파일(측정용 음성 샘플, 제3자 고지, 내려받은 엔진)
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
