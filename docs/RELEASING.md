# 릴리스 방법 (개발자용)

설치 파일은 키 없이 판매 플랫폼에서 파일로 배포한다(`docs/PLAN.md`의 판매·배포 결정). 태그를 푸시하면 GitHub Actions가 설치 파일을 만들어 **초안(draft) 릴리스**로 올린다. 자동으로 공개하지 않는다.

## 1. 버전 올리기

아래 세 파일의 버전을 같은 값으로 고친다. 하나라도 다르면 릴리스 워크플로가 한국어 오류로 멈춘다.

- `package.json`의 `version`
- `src-tauri/tauri.conf.json`의 `version`
- `src-tauri/Cargo.toml`의 `version`

그리고 락 파일을 맞춘다.

```bash
npm install --package-lock-only        # package-lock.json
cargo check --manifest-path src-tauri/Cargo.toml   # src-tauri/Cargo.lock
```

의존성(npm·Cargo)이 바뀌었으면 제3자 라이선스 전문도 다시 만든다.

```bash
npm run licenses:full
```

## 2. CHANGELOG 쓰기

`CHANGELOG.md` 맨 위(제목 설명 바로 아래)에 새 구역을 추가한다. 제목 형식은 정해져 있다.

```
## [0.13.0] - 2026-12-01
- 선생님이 눈으로 알아챌 변경만 짧게 (기능, 중요한 고침, 데이터·개인정보 관련 안내)
```

- 개발자 용어를 피하고 선생님 말투로 쓴다. 데이터 형식이 바뀌면 "업데이트하면 데이터가 자동으로 새 형식으로 바뀝니다"를 넣는다.
- 선생님이 첫 사용 때 확인해야 할 것이 있으면 적는다.
- 이 구역 본문이 릴리스 설명이 된다. 구역이 없거나 비어 있으면 `npm run validate:data`와 릴리스 워크플로가 실패한다.

## 3. 확인하고 커밋

```bash
npm run build
npm run validate:data
npm test
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

모두 통과한 뒤 커밋하고 `main`에 올려 `ci.yml`이 통과하는지 본다.

## 4. 태그 푸시

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

태그 이름은 `v` + 버전이다(예: `v0.13.0`). 이 푸시는 워크플로 세 개를 동시에 시작한다.

| 워크플로 | 하는 일 |
|---|---|
| `release.yml` | 버전 일치 확인 → 검증·테스트 → 설치 파일(NSIS) 빌드 → SHA-256 → **초안 릴리스** 생성 |
| `engines.yml` | 번들 엔진(whisper-cli, llama-bench, llama-server)이 실제로 도는지 확인 |
| `e2e.yml` | 윈도우 WebView2에서 앱을 띄워 주요 흐름 확인 |

셋 다 초록이 될 때까지 기다린다. `engines`나 `e2e`가 빨갛다면 그 설치 파일은 판매하지 않는다.

## 5. 초안 받아 판매 플랫폼에 올리기

1. GitHub 저장소의 Releases에서 초안을 연다. 설명에 CHANGELOG 구역이 들어 있는지 본다.
2. `tonghap-module_X.Y.Z_x64-setup.exe`와 `SHA256SUMS.txt`를 내려받는다.
3. 받은 exe의 해시가 `SHA256SUMS.txt`와 같은지 확인한다(PowerShell: `Get-FileHash <파일> -Algorithm SHA256`).
4. 판매 플랫폼에 새 설치 파일을 올리고, 선생님께 보일 설명에 CHANGELOG 구역을 옮긴다.
5. GitHub 릴리스는 공개(Publish)하거나 초안으로 둔다. 저장소가 공개라면 공개 릴리스는 누구나 설치 파일을 받을 수 있게 되므로, 판매 방식에 맞게 정한다.

## 다시 만들기

- 빌드가 실패했거나 설치 파일만 다시 만들고 싶을 때: Actions의 「릴리스 (초안)」에서 Run workflow → 태그 입력(예: `v0.13.0`). 같은 태그의 기존 **초안**을 갱신(파일 덮어쓰기, 설명 교체)한다.
- 이미 공개된 릴리스는 건드리지 않고 멈춘다. 고칠 것이 있으면 버전을 올려 새 태그를 만든다.
- 태그 뒤에 코드를 고쳐야 한다면 태그를 지우고 다시 만들지 말고(`engines`·`e2e`도 함께 다시 돈다) 새 패치 버전을 내는 편이 안전하다.

## 참고

- 코드 서명이 없어서 설치할 때 SmartScreen 경고가 뜬다. 안내는 `docs/INSTALL.md`. 인증서를 사면 `release.yml`의 빌드 단계 뒤에 서명 단계를 추가한다.
- 릴리스 워크플로는 `contents: write` 권한을 이 작업에만 가진다.
