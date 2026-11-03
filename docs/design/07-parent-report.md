# 설계 07: 학부모 리포트 내보내기

주제책(48차시)을 마칠 때 아이별로 "학교에서 무엇을 탐구하고, 실천하고, 놀았는지"를 한 장으로 정리해 가정에 보낸다. 서버가 없으므로 **이미지(PNG) 파일**로 만들고, 교사가 쓰던 알림장 앱으로 전달한다.

## 개인정보 원칙 (가장 중요)

- 리포트에는 **그 아이의 정보만** 들어간다.
- 여러 학생이 함께 기록된 관찰은 다른 학생 이름을 넣지 않는다("친구들과 함께"로 표시).
- 사진은 다른 아이 얼굴이 찍혀 있을 수 있으므로, **그 아이 한 명만 기록된 관찰의 사진만 기본 포함**한다. 여러 명이 함께 기록된 관찰의 사진은 기본 제외이고, 교사가 하나씩 켤 수 있다(켤 때 "다른 아이가 찍혀 있지 않은지 확인하세요" 안내).
- 음성은 넣지 않는다. 자막은 교사가 고른 경우에만 인용문으로 넣는다.
- 만든 파일은 교사가 고른 폴더에만 저장한다. 앱은 어디로도 보내지 않는다.

## 내용 (한 장, 세로 A4 비율, 1240×1754px)

1. 머리: 학교생활 리포트, 학년도·학년·반, 아이 이름, 주제책 제목과 기간(예: 「학교」 3월 4일~4월 12일).
2. **이렇게 배웠어요**: 그 기간 그 주제책 단원의 완료된 차시 제목 최대 8개(단원 빌더). 없으면 주제책 소주제.
3. **이런 모습을 보였어요**: 교사가 고른 관찰 최대 4개 — 날짜, 메모(그 아이 관련 문장 그대로, 120자 넘으면 줄임), 태그. 다른 학생 이름은 지우고 "친구들과 함께" 표시.
4. **사진**: 교사가 고른 사진 최대 4장(위 원칙).
5. **생활 습관**: 기간 중 습관 신호등 실천율(습관별 "N일 중 M일").
6. **선생님이 전하는 말**: 교사가 쓰는 글(최대 400자). 아이별로 다르게 쓸 수 있다.
7. 바닥: 「바슬즐 탐험대에서 만든 리포트입니다」 작은 글씨.

평가 지침에 따라 점수·등급·순위 표현은 넣지 않는다.

## 데이터 (마이그레이션 user_version 5)

```sql
CREATE TABLE parent_reports (
  id TEXT PRIMARY KEY,
  class_id TEXT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
  student_id TEXT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  topic_book_id TEXT,
  period_from TEXT NOT NULL,           -- 'YYYY-MM-DD'
  period_to TEXT NOT NULL,
  message TEXT NOT NULL DEFAULT '',
  observation_ids TEXT NOT NULL DEFAULT '[]',   -- JSON 배열(고른 관찰, 순서 유지)
  attachment_ids TEXT NOT NULL DEFAULT '[]',    -- JSON 배열(고른 사진)
  quote_attachment_ids TEXT NOT NULL DEFAULT '[]', -- 자막을 인용할 음성
  exported_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (student_id, topic_book_id, period_from, period_to)
);
```

## 명령

| 명령 | 입력 | 출력 |
|---|---|---|
| `report_draft` | `{ studentId, topicBookId?, from, to }` | `ReportDraft` — 저장된 리포트가 있으면 그것, 없으면 기본 선택으로 새로 만든 초안(저장은 안 함) |
| `report_save` | `{ input: ReportInput }` | `ReportDraft` |
| `report_mark_exported` | `{ id }` | — |
| `report_write_file` | 원시 PNG 본문 + 헤더 `x-path`(교사가 고른 저장 경로, UTF-8을 percent-encoding) | — |

- `ReportDraft`: 머리 정보, `lessons: string[]`, `candidates: { observation, soloForStudent, photoIds, audioWithTranscript }[]`(기간 안 그 아이 관찰 전체, 최신순), 선택 상태(관찰·사진·인용 id), `habits`, `message`.
- 기본 선택: 관찰은 메모가 있는 것 중 최신 4개(성취기준이 붙은 것 우선), 사진은 단독 관찰 사진 최신 4장, 인용 없음.
- `observation` 안의 학생 목록은 **그 아이만 남기고** 다른 학생은 `othersCount` 숫자로만 준다(프런트엔드가 다른 아이 이름을 아예 받지 않게).
- `report_write_file`: 확장자 `.png`만, 부모 폴더가 있어야 함, 30MB 이하. 파일 쓰기는 `.part` 후 이름 변경.

## 이미지 만들기 (프런트엔드)

- 리포트 컴포넌트를 화면 밖(고정 크기 1240×1754 div)에 그리고 `html-to-image`(npm, 최신 안정 버전 고정)의 `toPng`로 그린다. 사진은 `convertFileSrc` 주소라 같은 출처로 그려진다.
- 글꼴: 시스템 한글 글꼴(맑은 고딕). 외부 글꼴을 불러오지 않는다.
- 저장: `@tauri-apps/plugin-dialog`의 `save`(기본 이름 `<번호>-<이름>-<주제책>-리포트.png`) → `report_write_file`.
- **학급 일괄 만들기**: 폴더 하나를 고르면(`open({ directory: true })`) 학급 학생 전원 리포트를 기본 선택으로 만들어 그 폴더에 저장. 진행 막대, 하나가 실패해도 나머지 계속, 끝에 실패 목록. 저장된 리포트가 있으면 그 선택을 쓴다.

## 화면

- 포트폴리오 학생 화면에 「학부모 리포트」 버튼, 학급 현황 화면에 「학급 리포트 일괄 만들기」 버튼.
- 리포트 편집 화면: 왼쪽 설정(주제책, 기간 — 기본은 그 주제책 단원의 첫 차시~마지막 완료 차시 날짜, 없으면 최근 6주), 관찰 후보 목록(체크, 최대 4개, 다른 친구와 함께 기록된 건 표시), 사진 후보(체크, 함께 기록된 사진은 경고 아이콘), 자막 인용 체크, 선생님 말 입력. 오른쪽 실제 크기 비율 미리보기. 아래 「저장」, 「이미지로 내보내기」.

## 테스트 기준

Rust:
- 마이그레이션 5, 버전 4 백업 복원.
- `report_draft` 기본 선택 규칙(메모 있는 최신 4개·성취기준 우선, 단독 사진만 4장), 기간 밖·삭제된 관찰 제외.
- **다른 학생 이름이 응답 어디에도 없음**(함께 기록된 관찰의 직렬화된 JSON에 다른 학생 이름 문자열이 없음을 검사).
- 저장·재조회 시 선택 순서 유지, 다른 학생의 관찰·사진 id를 선택에 넣으면 거부.
- `report_write_file`: 확장자·크기·없는 폴더 거부, 기존 파일 덮어쓰기.
- 습관 실천율이 기간으로 계산됨.

프런트엔드: 스모크에서 미리보기 렌더링, 관찰 4개 초과 선택 막힘, 함께 기록된 사진 켤 때 경고, 일괄 만들기 진행·실패 목록.
