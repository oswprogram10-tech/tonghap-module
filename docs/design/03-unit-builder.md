# 설계 03: 단원 빌더

2022 개정 통합교과는 교과서를 참고 자료로 두고, 교사가 **단원 개발 도구 8종** 중 하나로 단원을 설계하며, 앞 차시의 학생 반응을 보고 다음 차시를 정하는 **꼬리 잇기** 운영을 허용한다.
단원 빌더는 이 설계·운영을 기록하고, 주제책별 필수 편성(안전 더하기 4차시, 놀이 8차시 이상, 메이킹북 3차시)과 성취기준 반영 상황을 보여 준다.
관찰 기록을 차시에 연결해, 나중에 평어 초안이 "어느 수업에서 무엇을 했는지"를 근거로 쓸 수 있게 한다.

## 범위

포함: 단원 만들기(주제책 + 도구 선택 → 4단계 자동 생성), 차시 추가·수정·순서 바꾸기·단계 옮기기·삭제, 성취기준 적용 방식(반복형·집중형·발현형) 표시, 차시 종류(일반·안전·놀이·메이킹북), 꼬리 잇기(차시를 마치며 학생 질문을 적고 다음 차시 만들기), 차시·시수 집계, 관찰 기록과 차시 연결.
제외: AI 활동 추천(로컬 모델 단계에서), 활동 카드 라이브러리, 인쇄·내보내기, 시간표 연동.

## 데이터 (마이그레이션 user_version 2)

```sql
CREATE TABLE units (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  topic_book_id TEXT,                  -- 교육과정 topicBooks[].id
  tool_id TEXT NOT NULL,               -- unitDevelopmentTools[].id 또는 'free'
  title TEXT NOT NULL,
  memo TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE INDEX idx_units_class ON units(class_id);

CREATE TABLE unit_stages (
  id TEXT PRIMARY KEY,
  unit_id TEXT NOT NULL REFERENCES units(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,           -- 0부터
  title TEXT NOT NULL
);

CREATE TABLE unit_sessions (
  id TEXT PRIMARY KEY,
  unit_id TEXT NOT NULL REFERENCES units(id) ON DELETE CASCADE,
  stage_id TEXT NOT NULL REFERENCES unit_stages(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,           -- 단계 안 순서, 0부터
  title TEXT NOT NULL,
  plan TEXT NOT NULL DEFAULT '',       -- 활동 내용
  minutes INTEGER NOT NULL DEFAULT 40 CHECK (minutes > 0),
  kind TEXT NOT NULL DEFAULT 'general' CHECK (kind IN ('general', 'safety', 'play', 'makingbook')),
  scheduled_on TEXT,                   -- 'YYYY-MM-DD'
  status TEXT NOT NULL DEFAULT 'planned' CHECK (status IN ('planned', 'done', 'skipped')),
  student_questions TEXT NOT NULL DEFAULT '',  -- 꼬리 잇기용 학생 질문·반응
  reflection TEXT NOT NULL DEFAULT '',
  follows_session_id TEXT REFERENCES unit_sessions(id) ON DELETE SET NULL,  -- 꼬리 잇기로 이어진 앞 차시
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_unit ON unit_sessions(unit_id);
CREATE INDEX idx_sessions_date ON unit_sessions(scheduled_on);

CREATE TABLE unit_session_standards (
  session_id TEXT NOT NULL REFERENCES unit_sessions(id) ON DELETE CASCADE,
  standard_code TEXT NOT NULL,
  mode TEXT NOT NULL CHECK (mode IN ('repeat', 'focus', 'emergent')),  -- 반복형·집중형·발현형
  PRIMARY KEY (session_id, standard_code)
);

ALTER TABLE observations ADD COLUMN unit_session_id TEXT REFERENCES unit_sessions(id) ON DELETE SET NULL;
CREATE INDEX idx_observations_session ON observations(unit_session_id);
```

- 마이그레이션 1은 고치지 않고 `MIGRATIONS`에 2번째 항목으로 추가한다. 백업 복원은 이미 낮은 버전을 마이그레이션하므로 그대로 동작해야 한다(테스트로 확인).
- 시수 환산: 1차시 = 40분. 화면의 차시 수 = 분 ÷ 40(소수 첫째 자리).
- 단계 제목은 프론트엔드가 교육과정 데이터의 `unitDevelopmentTools[].steps`(4개)를 보내 만든다. `free`(자유형)는 단계 1개 "수업"으로 시작. 단계 제목은 나중에 바꿀 수 있고, 단계 추가·삭제는 하지 않는다(단순하게 유지).
- 성취기준 코드·주제책 id 형식 검사는 관찰 기록과 같은 규칙을 공유한다(함수 재사용).

## Tauri 명령

| 명령 | 입력 | 출력 |
|---|---|---|
| `unit_list` | `{ classId, includeArchived }` | `UnitSummary[]` (최근 수정 순) |
| `unit_create` | `{ input: { classId, topicBookId?, toolId, title, stageTitles: string[] } }` | `UnitDetail` |
| `unit_update` | `{ id, title, memo, topicBookId? }` | `UnitDetail` |
| `unit_archive` / `unit_unarchive` | `{ id }` | — |
| `unit_delete` | `{ id }` | — (차시 함께 삭제, 연결된 관찰은 연결만 끊김) |
| `unit_get` | `{ id }` | `UnitDetail` |
| `stage_rename` | `{ id, title }` | — |
| `session_create` | `{ input: SessionInput }` | `Session` (단계 끝에 추가) |
| `session_update` | `{ id, input: SessionInput }` | `Session` |
| `session_move` | `{ id, stageId, position }` | `UnitDetail` (위치 재정렬) |
| `session_delete` | `{ id }` | — |
| `session_complete_and_follow` | `{ id, studentQuestions, reflection, next?: { title, stageId } }` | `UnitDetail` |
| `sessions_for_record` | `{ classId, date }` | `SessionOption[]` |

- `SessionInput`: `{ unitId, stageId, title, plan, minutes, kind, scheduledOn?, status, studentQuestions, reflection, standards: { code, mode }[] }`.
- `Session`: 입력 필드 + `id`, `position`, `followsSessionId`, `observationCount`(삭제 안 된 관찰 수), `createdAt`, `updatedAt`.
- `UnitDetail`: 단원 필드 + `stages: { id, position, title, sessions: Session[] }[]` + `totals`.
- `totals`: `{ sessions, doneSessions, minutes, doneMinutes, byKind: { general, safety, play, makingbook }(분), standards: { code, repeat, focus, emergent }[](차시 수) }`.
- `UnitSummary`: `{ id, title, topicBookId, toolId, updatedAt, archivedAt, totals }`.
- `session_complete_and_follow`: 차시를 `done`으로 바꾸고 질문·성찰을 저장한다. `next`가 있으면 새 차시를 만든다. 새 차시는 같은 단원, 지정한 단계의 **완료한 차시 바로 뒤**(같은 단계면) 또는 끝(다른 단계면)에 넣고, `followsSessionId` = 완료한 차시, `plan` 첫머리에 `"앞 차시 학생 질문:\n<질문>\n\n"`을 넣는다. 성취기준은 복사하지 않는다(발현형은 교사가 새로 고름). 한 트랜잭션.
- `sessions_for_record`: 그 학급의 보관 안 된 단원 차시 중 (1) `scheduledOn`이 그날인 것, (2) 그다음 상태가 `planned`인 차시(단원별 첫 번째)를 돌려준다. 각 항목 `{ sessionId, unitId, unitTitle, topicBookId, sessionTitle, scheduledOn, status, standardCodes }`. 관찰 기록 화면의 "차시" 선택지에 쓴다.
- 관찰 기록: `ObservationInput`에 `unitSessionId?` 추가. 저장 시 같은 학급 단원의 차시인지 검사. `Observation`에도 `unitSessionId`, `unitSessionTitle` 추가. 기존 테스트가 깨지지 않게 선택 필드로 둔다.

## 화면

상단 탭: **관찰 기록, 포트폴리오, 단원, 주제책, 기기 점검, 백업**. 기존 「단원 개발 도구」 탭은 없애고 그 내용(도구 8종·교수학습 모형)은 단원 만들기 화면에서 보여 준다.

### 단원 목록

- 현재 학급의 단원 카드: 제목, 주제책, 도구 이름, 진행(완료 차시/전체 차시), 시수(차시 환산 합계 / 주제책 48차시), 필수 편성 배지 3개(안전 n/4, 놀이 n/8, 메이킹북 n/3 — 채우면 초록, 모자라면 노랑).
- 「새 단원」, 보관함 보기 전환.

### 새 단원 만들기

1. 주제책 선택(현재 학급 학년의 8권, 학기별 묶음. 「주제책 없음」 허용).
2. 도구 선택: 8종 카드(이름 + 4단계) + 「자유형」. 카드 하나를 누르면 선택.
3. 제목(기본값: 주제책 제목). 「만들기」 → 단원 화면으로.

### 단원 화면

- 머리: 제목(수정), 주제책, 도구, 메모, 집계 막대(시수, 필수 편성 배지, 성취기준 반영 표: 이 주제책의 바·슬·즐 3개 + 그 밖에 쓰인 코드, 방식별 차시 수).
- 본문: 단계별 세로 목록(단계 제목 수정 가능). 각 단계에 차시 카드와 「차시 추가」.
  - 차시 카드: 순서 번호, 제목, 시간(40분/80분…), 날짜, 종류 배지, 성취기준 칩(코드 + 반·집·발 한 글자), 상태(계획·완료·건너뜀), 관찰 기록 수, 꼬리 잇기로 이어졌으면 "↳ 앞 차시에서 이어짐".
  - 순서 바꾸기는 드래그 없이 카드의 「위로」·「아래로」 버튼, 단계 옮기기는 차시 편집 창의 단계 선택.
- 차시 편집 창: 제목, 활동 내용(여러 줄), 시간(40·80·120분 버튼 + 직접 입력), 날짜, 종류, 상태, 성취기준(이 주제책 3개는 칩으로, 그 밖의 48개는 선택 목록으로 추가; 각 성취기준마다 반복형·집중형·발현형 선택), 학생 질문·반응, 성찰, 단계, 「삭제」.
- 「마치고 다음 차시 잇기」(계획 상태 차시에서): 학생 질문·반응과 성찰 입력 → 다음 차시 제목과 단계(기본: 같은 단계) → 저장하면 완료 처리 + 새 차시 생성 후 새 차시 편집 창을 연다. 「다음 차시 없이 마치기」도 둔다.

### 관찰 기록 화면 연동

- 주제책 선택 옆에 「차시」 선택(선택 사항): `sessions_for_record(오늘)` 결과. 오늘 날짜 차시가 하나면 자동 선택. 차시를 고르면 주제책을 그 단원 주제책으로 맞추고, 그 차시의 성취기준을 미리 선택(교사가 바꿀 수 있음).
- 포트폴리오 타임라인 카드에 차시 제목 표시.

## 테스트 기준 (Rust)

- 마이그레이션 2 적용: 버전 1 DB(기존 데이터 포함)에서 올렸을 때 관찰 데이터 유지, `unit_session_id` 컬럼 존재.
- 버전 1 스키마로 만든 백업을 복원하면 마이그레이션되어 열림(백업 테스트 헬퍼 재사용).
- 단원 만들기: 단계 4개(자유형 1개), 잘못된 도구 id·단계 수(0개, 9개 이상)·주제책 형식 거부, 다른 학급 접근 거부.
- 차시 만들기·수정: 성취기준 교체, 잘못된 코드·mode·kind·minutes(0 이하) 거부, 다른 단원의 단계로 지정 거부.
- `session_move`: 같은 단계 안 위치 바꾸기와 다른 단계로 옮기기 후 두 단계의 position이 0..n으로 연속.
- `session_complete_and_follow`: 완료 처리, 새 차시 위치(같은 단계 바로 뒤 / 다른 단계 끝), `followsSessionId`, plan 첫머리, 트랜잭션(잘못된 단계면 완료도 되돌려짐).
- 집계 `totals`: 분·차시·종류별·성취기준 방식별 개수, 완료만 따로.
- 단원 삭제 시 연결된 관찰은 남고 `unit_session_id`가 NULL.
- `sessions_for_record`: 그날 차시, 단원별 다음 계획 차시, 보관 단원 제외.
- 관찰 기록에 다른 학급 차시를 연결하면 거부.
