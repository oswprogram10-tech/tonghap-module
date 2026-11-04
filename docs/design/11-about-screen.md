# 설계 11: 「정보」 화면

판매 방식(키 없음, 판매 플랫폼에서 새 설치 파일로 업데이트, 코드 서명 없음)에 맞춰, 선생님이 앱 안에서 버전·데이터 위치·업데이트 방법·라이선스를 확인할 수 있게 한다. **인터넷에 접속하지 않는다.**

## 위치

상단 탭 맨 끝 「정보」.

## 내용

1. **앱**: 이름, 버전(`app_info`), 교육과정 데이터 버전, 놀이·활동 카드 수.
2. **내 데이터**
   - 데이터 폴더 경로(`app_local_data_dir()/data`)와 「폴더 열기」(opener 플러그인 `revealItemInDir` 또는 `openPath` — 필요한 권한만 추가).
   - 사용량: DB 파일(+ wal), 첨부 파일(사진·음성), 내려받은 모델 각각의 크기와 합계. 계산은 Rust 새 명령 `about_storage`(폴더를 걸으며 합산, `spawn_blocking`).
   - 개수: 학급, 학생, 관찰 기록, 첨부 수.
   - 마지막 백업 시각(`backup_status` 재사용)과 「백업 탭으로」 버튼.
   - 안내: "모든 기록은 이 PC에만 저장됩니다. 앱이 인터넷을 쓰는 것은 기기 점검에서 AI 모델을 내려받을 때뿐입니다."
3. **업데이트 방법**: `docs/INSTALL.md`의 「업데이트하기」와 같은 내용(먼저 백업 → 판매 페이지에서 새 설치 파일 → 덮어 설치, 기록·모델 유지 → 버전 확인). 코드에 박지 말고 `src/about/content.ts` 상수로 두어 문서와 함께 고치기 쉽게.
4. **설치 경고 안내**: 「Windows의 PC 보호」가 뜨는 이유와 「추가 정보 → 실행」.
5. **오픈소스와 모델 라이선스**: `src-tauri/resources/THIRD_PARTY_NOTICES.md` 내용을 그대로 보여 준다(빌드 때 `?raw`로 가져와 간단한 마크다운 표시 — 제목·표·코드 블록 정도만; 외부 마크다운 라이브러리를 새로 넣지 않는다). 프런트엔드 의존성(React, Tauri 등)의 라이선스 목록도 함께 보여 준다: 빌드 스크립트 `scripts/collect-licenses.mjs`가 `package.json`의 dependencies(직접 의존성)와 `cargo metadata`의 직접 의존성 이름·버전·라이선스를 모아 `src/about/licenses.json`을 만들고 커밋한다(빌드 때마다 네트워크 없이 만들 수 있어야 함; `cargo metadata --offline` 실패 시 기존 파일 유지).
6. **교육과정 출처**: 교육부 고시 제2022-33호 [별책 15], 성취기준 원문 확인 상태(`verification`) 요약.

## 테스트 기준

- Rust `about_storage`: 임시 폴더에 파일을 만들어 합계가 맞는지, 없는 폴더는 0.
- `collect-licenses.mjs`: 결과 JSON에 react, @tauri-apps/api, tauri, rusqlite가 이름·버전·라이선스와 함께 들어 있음(Node 테스트).
- 스모크: 정보 탭 렌더링, 폴더 열기 호출, 라이선스 섹션에 llama.cpp·Qwen3 표시, 콘솔 오류 없음.
