# 설계 21: 사진·음성 파일 암호화

설계 20으로 기록 DB는 암호화됐지만 `<로컬 데이터>/data/media/`의 사진·음성은 평문이다. 아이 얼굴과 목소리라 DB보다 민감할 수 있다. 같은 키 체계로 암호화하고, 화면은 복호화 통로로 읽는다.

## 1. 키와 파일 형식

- 미디어 키 = HKDF-SHA256(DB 키, salt 없음, info `"tonghap-media-v1"`) 32바이트. DB 키(`db.key`, DPAPI)를 그대로 쓰지 않고 용도별로 나눈다. DB가 잠김(설계 20)이면 미디어도 못 읽는다.
- 암호: XChaCha20-Poly1305(RustCrypto `chacha20poly1305`, MIT/Apache). 새 의존성 `chacha20poly1305`, `hkdf`(sha2는 있음). 라이선스 파일 재생성.
- 파일 형식(파일 하나를 통째로 암호화 — 사진·음성 메모는 수 MB라 충분):
  - `b"TGM1"`(4) + nonce(24, 난수) + 암호문+태그
  - AAD = 첨부의 `rel_path`(UTF-8). 파일을 다른 첨부 자리로 바꿔치기하면 열리지 않는다.
- 상한: 한 파일 64MB(넘으면 저장 거부, 지금 앱이 만드는 크기보다 훨씬 큼).
- 파일 이름과 확장자는 그대로(`2027/03/<id>.jpg`). 내용만 암호문.
- 읽을 때 앞 4바이트로 판별: `TGM1`이면 복호화, 아니면 평문으로 그대로(전환 중이거나 예전 파일).

## 2. 저장

- 첨부를 저장하는 모든 경로(관찰 사진·음성, 복원, 테스트 도우미 `seed_year`)가 암호화된 파일을 쓴다. 임시 파일에 쓰고 `rename`.

## 3. 화면에서 읽기 — 새 프로토콜

- Tauri `register_asynchronous_uri_scheme_protocol("tgmedia", …)`. 화면 주소: 윈도우 `http://tgmedia.localhost/<첨부 id>`, 그 밖 `tgmedia://localhost/<첨부 id>`(Tauri가 정한 형태를 그대로 따름; `src/lib/media.ts`의 `mediaUrl(id)` 한 곳에서 만든다).
- **경로가 아니라 첨부 id**를 받는다. DB에서 `rel_path`를 찾아 미디어 폴더 안인지 확인한 뒤 복호화해 돌려준다. 다른 파일은 읽을 수 없다(경로 탈출 테스트).
- 응답: `Content-Type`(mime 칼럼), `Cache-Control: no-store`, 음성은 `Accept-Ranges: bytes`와 `Range` 요청 처리(206) — `<audio>` 되감기용. 화면 저장(리포트 PNG의 `html-to-image`)이 쓸 수 있도록 같은 앱 출처에서 `fetch`가 되는지 확인(필요하면 `Access-Control-Allow-Origin`).
- `convertFileSrc(absPath)`를 쓰는 곳(`src/records/Attachments.tsx`, `src/stt/convert.ts`, `src/reports/ReportSheet.tsx`, `src/reports/ReportEditor.tsx`)을 `mediaUrl(id)`로 바꾼다. 소스 검사 테스트: `src/` 안에 `convertFileSrc`가 남지 않음.
- CSP(`tauri.conf.json`)에 새 출처를 img/media/connect에 추가하고, `asset:`/`assetProtocol`/`protocol-asset` 기능은 **제거**한다(디버그 `TONGHAP_DATA_DIR`용 asset 범위 허용 코드도 함께 정리).
- 자막: whisper용 임시 WAV는 지금처럼 `stt-tmp`에 평문으로 잠깐 만들고 끝나면 지운다(시작 시 남은 파일 정리 — 이미 있으면 확인). 원본 음성 파일은 암호문.

## 4. 기존 파일 전환 (업데이트 후 백그라운드)

- 앱 시작 후 DB가 `Ok`이면 백그라운드 스레드가 첨부를 하나씩:
  1. 평문이면(앞 4바이트가 `TGM1` 아님) 읽어 SHA-256 계산 → 암호화해 `<파일>.enc-tmp`에 쓰기 → 그 파일을 다시 복호화해 해시 비교 → 같으면 `rename`으로 원본 자리를 덮음(윈도우에서 덮어쓰기 rename이 되는 방법 사용 — `std::fs::rename`은 윈도우에서도 대상이 있으면 바꿔 줌. 확인 테스트).
  2. 실패하면 tmp를 지우고 원본은 그대로, 다음 시작에 다시.
- 판별이 파일마다라 중간에 꺼져도 안전하다(프로토콜은 평문·암호문 모두 읽음). 시작 시 남은 `*.enc-tmp` 정리.
- `before-restore-*` 폴더의 `media/`도 같은 방식(실패해도 계속).
- 진행 상황: 정보 화면 「기록 보호」에 「사진·음성 암호화 중 (N/M)」, 끝나면 「사진·음성도 암호화되어 있습니다」. 진단 파일 `about.json`에 `mediaEncrypted: 암호화된 수`, `mediaPlain: 평문 수`(숫자만).

## 5. 백업·복원 (형식 유지)

- 백업 보관함 안 `media/…`는 **평문** 그대로(보관함 전체가 age 암호화, 다른 PC에서 복원 가능, 예전 앱과 형식 같음). 만들 때 파일을 복호화하면서 tar에 쓰고, manifest의 해시는 평문 기준(지금과 같음).
- 복원: staging의 평문 미디어를 현재 미디어 키로 암호화한 뒤 swap(설계 20의 DB 처리와 같은 위치에서). 예전 백업(평문) 복원 테스트.

## 6. 문서

- MANUAL 「4. 개인정보와 데이터」, PRIVACY, 정보 화면 문구에서 「사진·음성은 아직 암호화되지 않습니다」를 갱신.

## 테스트 기준

- 형식: 왕복, 다른 AAD(rel_path)로는 실패, 손상(1바이트 바꿈) 실패, 평문 파일은 그대로 읽힘.
- 저장한 파일 바이트에 PNG/WAV 시그니처가 없음.
- 프로토콜: id로 읽기, 없는 id 404, 경로 탈출 불가, Range 206, mime.
- 전환: 평문 첨부 N개 → 모두 암호문, 내용 해시 같음. 실패 주입(쓰기 실패, 검증 불일치) → 원본 그대로. 중간 종료 흉내(tmp 남음) → 정리 후 계속.
- 백업 → 복원 왕복(암호화 미디어), 보관함 안 미디어는 평문, 예전 평문 백업 복원 후 암호문.
- E2E(윈도우): 기존 사진 그려짐(`naturalWidth > 0`), 음성 자막, 백업·복원, 리포트 PNG가 그대로 통과.
