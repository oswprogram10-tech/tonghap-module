# 설계 01: 관찰 기록과 학생별 포트폴리오

MVP-0의 첫 기능. 교사가 수업 중이나 직후에 **1분 안에** 관찰을 기록하고, 기록이 학생별 포트폴리오로 쌓이게 한다.
나중에 생기부 평어 초안(로컬 언어 모델)이 이 기록을 근거로 쓰므로, 기록은 학생·주제책·성취기준과 연결돼야 한다.

## 범위

포함:
- 학급과 학생 명단 관리(붙여넣기로 일괄 입력)
- 관찰 기록 작성·수정·삭제(휴지통): 학생(여러 명), 주제책, 성취기준, 활동 태그, 메모, 사진, 음성 메모
- 학생별 포트폴리오: 타임라인, 성취기준별 근거 개수
- 학급 현황: 학생 × 주제책 기록 개수(기록이 적은 학생 찾기)

제외(이후 설계): 음성 자동 자막(whisper), 백업·복원, 데이터 암호화, 웹캠 촬영, 학부모 리포트, 생기부 평어.
음성 메모에는 나중에 자막을 넣을 `transcript` 칸만 만들어 둔다.

## 저장 방식

- SQLite. Rust에서 `rusqlite`(`bundled` 기능)로 직접 다룬다. 프론트엔드는 Tauri 명령만 호출한다(SQL을 프론트엔드에 두지 않음).
- 파일 위치: `app_local_data_dir()/data/tonghap.db`, 첨부 파일은 `app_local_data_dir()/data/media/YYYY/MM/<uuid>.<ext>`.
- DB 연결은 `tauri::State<Mutex<rusqlite::Connection>>` 하나로 관리한다. `PRAGMA foreign_keys = ON`, `PRAGMA journal_mode = WAL`.
- 마이그레이션: `PRAGMA user_version`으로 버전 관리. `src-tauri/src/db/migrations.rs`에 순서대로 SQL을 두고 앱 시작 시 적용.
- ID는 UUID v4 문자열. 시각은 로컬 시간 ISO 8601 문자열(`2027-03-05T10:20:00+09:00`).

### 스키마 (user_version 1)

```sql
CREATE TABLE classes (
  id TEXT PRIMARY KEY,
  school_year INTEGER NOT NULL,          -- 학년도, 예: 2027
  grade INTEGER NOT NULL CHECK (grade IN (1, 2)),
  name TEXT NOT NULL,                    -- 예: '2반'
  created_at TEXT NOT NULL,
  archived_at TEXT
);

CREATE TABLE students (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  number INTEGER NOT NULL,               -- 출석 번호
  name TEXT NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,     -- 전학 등은 0 (기록은 남김)
  created_at TEXT NOT NULL,
  UNIQUE (class_id, number)
);

CREATE TABLE observations (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  observed_at TEXT NOT NULL,
  topic_book_id TEXT,                    -- 교육과정 데이터의 topicBooks[].id, 없을 수 있음
  note TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT                        -- 휴지통
);
CREATE INDEX idx_observations_class_time ON observations(class_id, observed_at);

CREATE TABLE observation_students (
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  PRIMARY KEY (observation_id, student_id)
);
CREATE INDEX idx_obs_students_student ON observation_students(student_id);

CREATE TABLE observation_standards (
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  standard_code TEXT NOT NULL,           -- 예: '[2슬02-04]'
  PRIMARY KEY (observation_id, standard_code)
);

CREATE TABLE observation_tags (
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  tag TEXT NOT NULL,
  PRIMARY KEY (observation_id, tag)
);

CREATE TABLE attachments (
  id TEXT PRIMARY KEY,
  observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('photo', 'audio')),
  rel_path TEXT NOT NULL,                -- media 폴더 기준 상대 경로, '/' 구분
  mime TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  transcript TEXT,                       -- 음성 자막(이후 whisper)
  created_at TEXT NOT NULL
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL                    -- JSON
);
```

- 성취기준 코드와 주제책 id는 교육과정 JSON을 기준으로 하므로 외래 키를 두지 않는다. 저장할 때 Rust에서 형식만 검사한다(`^\[2.{1,2}\d{2}-\d{2}\]$`, `^g[1-6]s[12]-[a-z0-9-]+$`).
- 관찰을 완전히 지우는 것(휴지통 비우기)은 첨부 파일도 디스크에서 삭제한다.

## 활동 태그 (고정 목록, 프론트엔드 상수)

`실천`, `탐구`, `놀이`, `표현`, `협력`, `배려`, `질문`, `안전`, `습관`.
바른 생활(실천)·슬기로운 생활(탐구)·즐거운 생활(놀이·표현)의 성격과 평어 작성에 자주 쓰이는 관점이다.

## Tauri 명령

모든 명령은 오류를 한국어 문자열로 돌려준다(`Result<T, String>`). 이름은 `snake_case`, 응답 필드는 `camelCase`.

| 명령 | 입력 | 출력 |
|---|---|---|
| `class_list` | — | `ClassInfo[]` (보관 안 된 것, 학년도·학년·이름 순) |
| `class_create` | `{ schoolYear, grade, name }` | `ClassInfo` |
| `class_update` | `{ id, schoolYear, grade, name }` | `ClassInfo` |
| `class_archive` | `{ id }` | — |
| `student_list` | `{ classId, includeInactive }` | `Student[]` (번호 순) |
| `student_import` | `{ classId, text }` | `{ added, updated, skipped: string[] }` |
| `student_update` | `{ id, number, name, active }` | `Student` |
| `observation_create` | `ObservationInput` | `Observation` |
| `observation_update` | `{ id, ...ObservationInput }` | `Observation` |
| `observation_delete` | `{ id }` | — (휴지통으로) |
| `observation_restore` | `{ id }` | — |
| `observation_purge` | `{ id }` | — (완전 삭제, 첨부 파일 포함) |
| `observation_list` | `ObservationQuery` | `Observation[]` (최신순) |
| `attachment_add_file` | `{ observationId, path }` | `Attachment` (사진 파일 복사) |
| `attachment_add_bytes` | 원시 바이트 본문 + 헤더 | `Attachment` (녹음 저장) |
| `attachment_delete` | `{ id }` | — |
| `portfolio_student` | `{ studentId }` | `StudentPortfolio` |
| `portfolio_class` | `{ classId }` | `ClassOverview` |
| `settings_get` / `settings_set` | `{ key }` / `{ key, value }` | JSON 값 |

- `student_import`의 `text`는 한 줄에 한 명. `번호 이름`, `번호<TAB>이름`, `번호. 이름`, `이름만`(번호는 기존 최대값 다음부터) 형식을 받는다. 같은 번호가 있으면 이름을 바꾼다(`updated`). 해석할 수 없는 줄은 `skipped`.
- `ObservationInput`: `{ classId, observedAt, topicBookId?, note, studentIds: string[], standardCodes: string[], tags: string[] }`. 학생은 1명 이상 필수, 같은 학급 학생인지 검사.
- `ObservationQuery`: `{ classId, studentId?, topicBookId?, standardCode?, includeDeleted?, limit?, before? }`. `before`는 `observedAt` 기준 페이지 넘김.
- `Observation`: 입력 필드 + `id`, `createdAt`, `updatedAt`, `deletedAt`, `students: {id, number, name}[]`, `attachments: Attachment[]`.
- `Attachment`: `{ id, kind, relPath, absPath, mime, bytes, transcript, createdAt }`. `absPath`는 화면에서 `convertFileSrc`로 표시하는 데 쓴다.
- `attachment_add_bytes`는 큰 녹음 파일을 JSON 배열로 보내지 않도록 `tauri::ipc::Request`의 원시 본문(`InvokeBody::Raw`)으로 받는다. 헤더: `x-observation-id`, `x-kind`(`audio`), `x-mime`(예: `audio/webm;codecs=opus`). 확장자는 MIME에서 정한다(webm, ogg, m4a, wav).
- 사진 파일은 jpg, jpeg, png, webp, heic만 받는다. 50MB 넘는 파일은 거부.
- `StudentPortfolio`: `{ student, observationCount, byTopicBook: {topicBookId, count}[], byStandard: {standardCode, count}[], byTag: {tag, count}[], firstObservedAt, lastObservedAt }`. 타임라인 자체는 `observation_list`의 `studentId`로 가져온다.
- `ClassOverview`: `{ students: {student, total, byTopicBook: Record<topicBookId, number>, lastObservedAt}[] }`. 삭제된 관찰은 세지 않는다.

## 화면

상단 탭에 **「관찰 기록」**, **「포트폴리오」**를 추가한다(기존: 주제책, 단원 개발 도구, 기기 점검).
학급이 하나도 없으면 두 화면 모두 학급 만들기와 명단 붙여넣기를 먼저 보여 준다.
현재 학급과 현재 주제책은 `settings`(`currentClassId`, `currentTopicBookId`)에 저장해 다음 실행 때 유지한다.

### 관찰 기록 (빠른 기록)

한 화면에서 위에서 아래로 탭만으로 끝나게 한다.

1. 학급 선택(학급이 둘 이상일 때), 주제책 선택(현재 학년의 4개 학기 주제책 중), 날짜·시각(기본 지금).
2. **학생 칩**: 번호와 이름이 있는 버튼 격자(64px 이상). 여러 명 선택 가능. 「모두 선택」, 「선택 해제」.
3. **성취기준 칩**: 선택한 주제책의 바·슬·즐 성취기준 3개. 코드와 원문 앞부분을 보여 준다. 선택 사항.
4. **태그 칩**: 고정 목록에서 여러 개.
5. 메모(여러 줄 입력).
6. 첨부: 「사진 추가」(파일 선택, 여러 장) → 미리보기, 「음성 메모」(녹음 시작/정지, MediaRecorder) → 재생 가능한 목록.
7. 「저장」: 저장 후 학생·성취기준·태그·메모·첨부는 비우고, 학급·주제책은 유지. 저장 완료를 잠깐 표시.

저장 순서: `observation_create` → 첨부 각각 `attachment_add_*`. 첨부 실패 시 관찰은 남기고 어느 첨부가 실패했는지 보여 준다.

아래쪽에 **최근 기록** 목록(이 학급 최신 20개): 날짜, 학생 이름들, 주제책, 태그, 메모 앞부분, 첨부 개수. 누르면 수정 창(같은 양식). 수정 창에 「삭제」(휴지통).

### 포트폴리오

- 왼쪽: 학생 목록(번호·이름·기록 수). 맨 위에 「학급 현황」 항목.
- **학급 현황**: 표. 행 = 학생, 열 = 현재 학년의 주제책 8권(학기별로 묶음) + 합계 + 마지막 기록일. 0인 칸은 강조해 기록이 비는 학생을 찾게 한다.
- **학생 포트폴리오**:
  - 요약: 총 기록 수, 첫·마지막 기록일, 태그별 개수.
  - 성취기준 격자: 4영역 × 4번호를 행, 바·슬·즐을 열로 하는 16×3 표에 근거 개수. 현재 학년 주제책에 해당하는 행만 진하게. 칸을 누르면 그 성취기준으로 타임라인을 거른다.
  - 주제책 필터 + 타임라인: 기록 카드(날짜, 주제책, 성취기준 코드, 태그, 메모 전문, 사진 썸네일, 음성 재생). 무한 스크롤 대신 「더 보기」 버튼(`before` 페이지 넘김).
  - 휴지통 보기 전환(삭제된 기록 복원·완전 삭제).

### 학급·명단 관리

포트폴리오 화면 상단의 「학급 관리」 버튼 → 학급 만들기·이름 바꾸기·보관, 명단 붙여넣기(미리보기 후 반영), 학생별 번호·이름 수정, 전학 처리(active=0).

## 첨부 파일 표시

- `tauri` 크레이트에 `protocol-asset` 기능을 켜고, `tauri.conf.json`의 `app.security.assetProtocol`을 `enable: true`, `scope: ["$APPLOCALDATA/data/media/**"]`로 설정한다.
- 화면에서는 `convertFileSrc(absPath)`로 `<img>`, `<audio>`에 넣는다. CSP의 `img-src`, `media-src`에 이미 `asset:`과 `http://asset.localhost`가 있다.
- 녹음은 `getUserMedia({ audio: true })`. 마이크 권한이 거부되면 안내 문구를 보여 준다.

## 테스트 기준

- Rust 단위 테스트(메모리 DB `Connection::open_in_memory()`):
  - 마이그레이션 적용과 `user_version`
  - `student_import`의 줄 형식 4가지, 번호 중복 시 이름 갱신, 잘못된 줄 skipped
  - 관찰 생성·수정(학생·성취기준·태그 교체), 다른 학급 학생이면 거부, 학생 0명이면 거부, 잘못된 성취기준 코드 거부
  - 삭제·복원·목록에서 제외, 완전 삭제 시 첨부 행 삭제
  - `portfolio_student`, `portfolio_class` 개수 집계(삭제된 관찰 제외)
- DB 로직은 `AppHandle` 없이 `&Connection`을 받는 함수로 분리해 테스트하고, Tauri 명령은 얇게 감싼다.
- `npm run build`, `npm run validate:data`, `cargo test`, `cargo clippy --all-targets` 통과.
