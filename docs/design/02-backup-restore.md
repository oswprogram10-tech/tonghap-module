# 설계 02: 백업과 복원

관찰 기록·명단·첨부 파일은 교사 PC에만 있으므로, PC 고장·교체·학년 말 이동에 대비해 **암호화된 백업 파일 하나**로 내보내고 되살릴 수 있어야 한다.

## 범위

포함: 수동 백업(암호 필수), 백업 파일 내용 미리보기, 복원(현재 데이터는 보관 폴더로 옮겨 둠), 백업 알림 배너.
제외: 자동 백업(암호를 저장해야 하므로 하지 않음), DB 자체 암호화(별도 설계), 클라우드 업로드(원칙상 하지 않음).

## 백업 파일 형식

- 확장자 `.tonghap`. 기본 파일 이름 `바슬즐-백업-YYYYMMDD-HHMM.tonghap`.
- 구조: **tar 묶음을 age(암호 기반, scrypt)로 암호화**한 스트림. 압축은 하지 않는다(사진·음성은 이미 압축돼 있음).
  - 크레이트: `age`(암호 방식 `age::scrypt`), `tar`. 둘 다 스트리밍이라 큰 첨부도 메모리에 다 올리지 않는다.
- tar 안의 항목 순서(반드시 이 순서):
  1. `manifest.json`
  2. `tonghap.db` — `VACUUM INTO`로 만든 일관된 스냅숏
  3. `media/YYYY/MM/<파일>` — 첨부 파일 전부
- `manifest.json`:

```json
{
  "format": "tonghap-backup",
  "formatVersion": 1,
  "appVersion": "0.3.0",
  "schemaVersion": 1,
  "createdAt": "2027-03-05T16:20:00+09:00",
  "counts": { "classes": 1, "students": 25, "observations": 120, "attachments": 45 },
  "files": [
    { "path": "tonghap.db", "bytes": 123456, "sha256": "..." },
    { "path": "media/2027/03/xxxx.webm", "bytes": 4567, "sha256": "..." }
  ]
}
```

- `files`에는 db와 모든 첨부 파일의 크기·SHA-256을 넣는다. 매니페스트를 먼저 쓰려면 해시를 미리 알아야 하므로, 백업은 (1) db 스냅숏을 임시 폴더에 만들고 (2) db와 첨부 파일 해시를 계산한 뒤 (3) tar를 쓴다.

## 백업 절차 (`backup_create`)

입력: `{ path, password }`, 진행 상황 `Channel<BackupProgress>`.

1. 암호 검사: 8자 이상(공백만은 불가). 화면에서 확인 입력과 일치 여부를 먼저 본다.
2. DB 잠금을 잡은 짧은 시간 동안 `VACUUM INTO '<임시폴더>/tonghap.db'`와 counts 조회. 잠금은 바로 놓는다.
3. 첨부 파일 목록은 DB의 `attachments.rel_path` 기준(디스크에만 있고 DB에 없는 파일은 넣지 않음). DB에 있는데 파일이 없으면 매니페스트 `missing` 배열에 적고 계속 진행한다.
4. 해시 계산 → tar 작성 → age 암호화 → `<path>.part`에 쓴 뒤 끝나면 `<path>`로 이름 변경. 실패하면 `.part`를 지운다.
5. 성공 시 settings `lastBackup` = `{ at, path, observations }` 저장.
6. 진행 상황: `{ stage: 'snapshot' | 'hashing' | 'writing', done, total }` (바이트 단위).

시간이 걸리는 일은 `async` 명령 + `spawn_blocking`으로 하고 UI 스레드를 막지 않는다.

## 미리보기 (`backup_inspect`)

입력 `{ path, password }` → 복호화해서 tar의 첫 항목 `manifest.json`만 읽고 멈춘다. 출력: 매니페스트(파일 목록 제외, 파일 수·총 바이트 추가).
오류 문구 구분: 암호가 틀림 / 이 앱의 백업 파일이 아님 / 파일이 손상됨 / 더 새 버전 앱의 백업(schemaVersion이 현재보다 큼).

## 복원 (`backup_restore`)

입력 `{ path, password }`, 진행 `Channel<BackupProgress>`(`stage: 'reading' | 'verifying' | 'swapping'`).

1. 복호화하며 tar를 `data/.restore-staging/`에 푼다. 항목 경로는 `manifest.json`, `tonghap.db`, `media/` 아래 일반 파일만 허용한다. 절대 경로, `..`, 심볼릭 링크, 그 밖의 이름이 있으면 중단(손상 또는 변조로 간주).
2. 매니페스트의 모든 파일 크기·SHA-256 확인. 매니페스트에 없는 파일이나 빠진 파일이 있으면 중단.
3. 풀린 db를 열어 `PRAGMA integrity_check`가 `ok`인지, `user_version` ≤ 현재 스키마인지 확인. 낮으면 마이그레이션을 적용한다.
4. 교체: DB 잠금을 잡고, 현재 연결을 메모리 DB로 바꿔 파일을 놓은 뒤,
   - 현재 `data/tonghap.db*`(wal, shm 포함)와 `data/media`를 `data/before-restore-YYYYMMDD-HHMMSS/`로 옮긴다.
   - staging의 db와 media를 `data/`로 옮긴다.
   - 새 db를 열어(`db::open`과 같은 설정) 잠금 안의 연결과 바꾼다.
   - 실패하면 보관 폴더에서 원래대로 되돌리고 원래 db를 다시 연다.
5. `before-restore-*` 폴더는 최신 3개만 남기고 지운다.
6. 성공 후 화면은 학급·기록을 다시 불러온다(현재 학급 설정이 없는 학급을 가리키면 첫 학급으로).

`MediaDir` 경로는 그대로이므로(`data/media`) 관리 상태는 바꾸지 않아도 된다.

## 화면

상단 탭 맨 끝에 **「백업」** 탭 추가.

- **백업 만들기**: 마지막 백업 시각과 그 뒤 새 기록 수 표시. 암호, 암호 확인 입력 → 「백업 파일 저장」 → 저장 위치 대화상자(`@tauri-apps/plugin-dialog`의 `save`, 필터 `.tonghap`) → 진행 막대 → 완료 시 경로와 「폴더 열기」.
  - 안내 문구: "암호를 잊으면 누구도 복원할 수 없습니다. USB나 개인 저장 공간 등 이 PC가 아닌 곳에 보관하세요."
- **복원하기**: 「백업 파일 선택」 → 암호 입력 → 「내용 확인」(미리보기: 만든 날짜, 앱 버전, 학급·학생·기록·첨부 수, 총 크기) → 「이 백업으로 복원」 → 확인 대화상자("현재 데이터는 보관 폴더로 옮겨 두고 백업 내용으로 바꿉니다") → 진행 막대 → 완료.
- **알림 배너**(관찰 기록 화면 위쪽): 마지막 백업이 없거나 7일이 지났고, 그 뒤 새 기록이 있으면 "마지막 백업 후 기록 N개가 늘었습니다" + 「지금 백업」(백업 탭으로 이동). 「오늘은 닫기」는 그날만 숨긴다(settings `backupReminderDismissedOn`).

권한: `capabilities/default.json`에 `dialog:allow-save` 추가.

## Tauri 명령

| 명령 | 입력 | 출력 |
|---|---|---|
| `backup_create` | `{ path, password, onProgress }` | `BackupResult { path, bytes, counts, missing }` |
| `backup_inspect` | `{ path, password }` | `BackupSummary` |
| `backup_restore` | `{ path, password, onProgress }` | `BackupSummary` |
| `backup_status` | — | `{ lastBackup: {at, path, observations} \| null, observationsSince: number }` |

## 테스트 기준 (Rust, 임시 폴더)

- 데이터(학급·학생·관찰·첨부 파일 2개)를 만든 뒤 백업 → 다른 data 폴더로 복원 → 개수와 첨부 파일 바이트가 같음.
- 틀린 암호 → "암호가 틀림" 오류. 무작위 바이트 파일 → "이 앱의 백업 파일이 아님" 계열 오류.
- 암호문 중간 바이트 변조 → 오류, 현재 데이터 변경 없음.
- `..` 경로나 매니페스트에 없는 파일이 든 tar(테스트에서 직접 만든 것) → 복원 거부.
- 매니페스트 해시 불일치 → 복원 거부.
- schemaVersion이 현재보다 큰 백업 → 거부.
- 복원 후 `before-restore-*` 폴더에 이전 데이터가 있고, 4번 복원하면 3개만 남음.
- 테스트에서는 scrypt 작업량을 낮춰 빠르게 돈다(`age::scrypt::Recipient::set_work_factor` 등 크레이트가 제공하는 방법 사용, 실제 앱은 기본값).
- DB 교체 로직은 `Mutex<Connection>`과 data 폴더 경로를 받는 함수로 분리해 테스트한다.
