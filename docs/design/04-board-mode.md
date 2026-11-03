# 설계 04: 칠판 모드

즐거운 생활의 "주 2회 이상 실외놀이·신체활동"과 주제책마다 8차시 이상의 놀이 수업, 바른 생활의 "일주일 이상 지속 실천"을 돕는 **전자칠판용 화면**.
원칙: 화면은 놀이를 **시작시키고 정리하는 데만** 쓴다(규칙 확인·팀 나누기·타이머 1~2분). 놀이 자체는 화면 밖에서 한다.

## 범위

포함: 칠판 창 열기, 놀이 룰렛(놀이 카드), 팀 나누기·술래 뽑기, 타이머, 습관 신호등(학생이 칠판에서 자기 이름을 눌러 체크), 습관 주간 보기, 포트폴리오에 습관 실천율.
제외: 배경음악(저작권), 카메라·동작 인식, 학생 개인 기기.

## 칠판 창

- 메인 창 상단 오른쪽에 「칠판 모드」 버튼. 누르면 `WebviewWindow`(label `board`, 제목 "바슬즐 칠판")를 연다. 이미 열려 있으면 앞으로 가져온다.
- 모니터가 둘 이상이면(전자칠판이 확장 모니터로 연결된 경우) 주 모니터가 아닌 쪽에 띄우고 전체 화면. 하나면 일반 창(최대화)으로 열고 칠판 창 안의 「전체 화면」 버튼으로 전환.
- 같은 프런트엔드 번들을 쓰고, URL 해시(`#/board`)나 창 label로 칠판 화면을 그린다.
- 권한: `capabilities/default.json`의 `windows`에 `board`를 넣고, 창 생성·전체 화면·포커스에 필요한 `core:window:*`, `core:webview:allow-create-webview-window` 권한만 추가한다.
- 칠판 화면은 현재 학급(`settings.currentClassId`)을 쓴다. 메인 창에서 학급을 바꾸면 칠판 창에 이벤트(`emit`/`listen`, 이벤트 이름 `board://class-changed`)로 알리고 다시 불러온다.

### 칠판 화면 공통 규칙

- 멀리서 보이게: 본문 글자 32px 이상, 제목 56px 이상, 버튼 최소 96px 높이, 고대비.
- 위쪽에 큰 탭 4개: **놀이**, **팀 나누기**, **타이머**, **습관 신호등**. 오른쪽에 「전체 화면」, 「닫기」.
- 어두운 배경 대신 밝은 배경(교실 조명에서 잘 보임). 색만으로 구분하지 않고 글자·기호를 함께 쓴다(신호등은 ●와 "했어요/아직").

## 1. 놀이 룰렛

- 놀이 카드: `data/plays/plays.json`(스키마 `data/plays/schema/plays.schema.json`, 검증 `npm run validate:data`). 교사가 만든 놀이는 DB `custom_plays`에 같은 모양으로 저장.
- 거르기: 장소(실외·실내·전체), 현재 주제책에 어울리는 것만(`topicBookIds`에 현재 주제책 포함), 15분 이하.
- 「돌리기」: 걸러진 놀이 이름이 2~3초 동안 빠르게 바뀌다 멈추는 애니메이션(효과음은 Web Audio로 짧은 비프, 끄기 가능). 같은 놀이가 연속으로 나오지 않게 한다. 「다시」, 「이 놀이로」.
- 「이 놀이로」 → 놀이 카드 전체 화면: 이름, 장소·인원·시간, 규칙(번호 매긴 큰 글씨), **안전 수칙**(노란 띠), 준비물. 아래에 「팀 나누기로」, 「타이머로(놀이 시간 자동 설정)」 버튼.
- 놀이 목록 보기(카드 격자)에서 직접 고를 수도 있다.
- 교사 놀이 추가·수정·삭제는 **메인 창**의 단원 탭이 아니라 칠판 모드 안 「놀이 관리」(교사용, 작은 글씨 허용)에서 한다.
- 놀이를 하면 기록: `play_log`(언제, 어떤 놀이, 학급, 분). 단원 빌더와는 이번에 연결하지 않는다(나중에 놀이 차시 집계에 쓸 수 있게 저장만).

## 2. 팀 나누기·술래 뽑기

- 현재 학급의 재학 학생 이름 버튼 격자. 처음엔 모두 참여. 오늘 빠진 학생은 눌러서 제외(흐리게 + "빠짐"). 제외 상태는 그날만 유지(settings `boardAbsent` = `{ date, studentIds }`).
- 팀 수 2~6 선택 → 「나누기」: 무작위로 고르게 나눈다(인원 차이 최대 1). 팀마다 색과 이름(1팀, 2팀… 또는 동물 이름)으로 큰 카드. 「다시 나누기」.
- 술래 뽑기: 인원 1~5 → 「뽑기」: 이름이 빠르게 바뀌다 멈춤. 최근 3번 안에 뽑힌 학생은 덜 뽑히게(가중치) — 같은 아이만 계속 술래가 되지 않게.
- 무작위는 프런트엔드 `crypto.getRandomValues`. 결과는 저장하지 않는다.

## 3. 타이머

- 프리셋 1·3·5·10·15분 + 직접 입력(분·초 큰 버튼 +/−). 큰 숫자 카운트다운, 원형 진행 표시.
- 시작·일시정지·재설정. 끝나기 10초 전 숫자 강조, 끝나면 화면 깜빡임 + 비프 3번(Web Audio, 끄기 가능).
- 놀이 카드에서 넘어오면 그 놀이의 `minutes`로 설정.
- 타이머가 도는 동안 다른 탭으로 가도 위쪽에 남은 시간이 작게 보인다.

## 4. 습관 신호등

바른 생활 평가의 "일주일 이상 자기 성찰 체크"를 학급 단위로 한다.

- 습관: 학급마다 여러 개(예: 사물함 정리하기, 고운 말 쓰기). 이름 + 이모지 아이콘 1개(선택). 기간(시작일, 끝일 선택)과 활성 여부.
- 칠판 화면: 습관 하나를 고르면 학생 이름 버튼 격자. 학생이 **자기 이름을 눌러** "했어요"(초록 ●)로 바꾸고, 다시 누르면 "아직"(회색 ○). 날짜는 오늘(교사가 어제로 바꿀 수 있음, 최근 7일까지).
- 위쪽에 오늘 실천 수/전체(예: 18/24명).
- 주간 보기: 학생 × 최근 7일 표(●/○), 학생별 7일 중 실천 일수. 교사용이므로 메인 창 포트폴리오에서도 본다.
- 포트폴리오 학생 화면: 습관별 최근 28일 실천율(실천한 날 ÷ 기록된 날 수가 아니라 ÷ 습관 기간 중 학교에 체크가 한 번이라도 있었던 날 수).

## 데이터 (마이그레이션 user_version 3)

```sql
CREATE TABLE custom_plays (
  id TEXT PRIMARY KEY,
  data TEXT NOT NULL,                  -- 놀이 카드 JSON(스키마의 항목 하나와 같은 모양, id는 'custom-<uuid>')
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE play_log (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  play_id TEXT NOT NULL,
  played_at TEXT NOT NULL,
  minutes INTEGER
);

CREATE TABLE habits (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  icon TEXT NOT NULL DEFAULT '',
  starts_on TEXT,
  ends_on TEXT,
  active INTEGER NOT NULL DEFAULT 1,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE habit_checks (
  habit_id TEXT NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  checked_on TEXT NOT NULL,            -- 'YYYY-MM-DD'
  done INTEGER NOT NULL CHECK (done IN (0, 1)),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (habit_id, student_id, checked_on)
);
```

## Tauri 명령

| 명령 | 입력 | 출력 |
|---|---|---|
| `play_custom_list` | — | `Play[]` |
| `play_custom_save` | `{ play }` (id 없으면 새로) | `Play` |
| `play_custom_delete` | `{ id }` | — |
| `play_log_add` | `{ classId, playId, minutes? }` | — |
| `habit_list` | `{ classId, includeInactive }` | `Habit[]` |
| `habit_save` | `{ input: { id?, classId, title, icon, startsOn?, endsOn?, active } }` | `Habit` |
| `habit_delete` | `{ id }` | — |
| `habit_day` | `{ habitId, date }` | `{ habit, date, entries: { studentId, number, name, done }[], doneCount, total }` |
| `habit_toggle` | `{ habitId, studentId, date }` | `{ done, doneCount, total }` |
| `habit_week` | `{ habitId, endDate }` | `{ dates: string[7], rows: { studentId, number, name, marks: (bool\|null)[7], doneDays }[] }` |
| `habit_student_summary` | `{ studentId, endDate, days }` | `{ habitId, title, icon, doneDays, schoolDays }[]` |

- 놀이 카드 검증은 Rust에서도 한다(필수 필드, 길이, 열거값). 스키마와 같은 규칙.
- `habit_toggle`: 학생이 그 습관 학급의 재학생인지, 날짜가 오늘부터 7일 전까지인지(미래 불가) 검사.
- `schoolDays`: 습관 기간 안에서 그 습관에 체크가 한 번이라도 있었던 날의 수. 그날 체크하지 않은 학생은 "아직".

## 테스트 기준 (Rust)

- 마이그레이션 3 적용(버전 2 DB의 데이터 유지), 버전 2 백업 복원 시 자동 마이그레이션.
- 놀이: 사용자 놀이 저장·수정·삭제, 잘못된 놀이(규칙 1개, 이름 21자, 없는 장소 값) 거부, 놀이 기록 추가.
- 습관: 저장·수정·비활성, 다른 학급 학생 체크 거부, 미래 날짜·8일 전 거부, 토글 두 번이면 원래대로, `habit_day` 개수, `habit_week` 7칸(체크 없는 날 null), `habit_student_summary` 실천율 분모(schoolDays) 계산, 전학생(active=0)은 `habit_day`에서 제외.
- 팀 나누기·술래 가중치 로직은 프런트엔드 순수 함수(`src/board/random.ts`)로 두고, 같은 입력·시드로 결과를 재현할 수 있게 난수 함수를 주입받는다. Node 내장 테스트 러너(`node --test`)로 팀 인원 차이 ≤ 1, 제외 학생 미포함, 가중치(최근 뽑힌 학생 확률 감소)를 검사하고 `npm test` 스크립트로 실행한다.
