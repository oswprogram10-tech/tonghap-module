// src-tauri/resources/THIRD_PARTY_LICENSES.md 를 만든다(npm run licenses:full).
// 구성: 머리말 -> 손으로 쓴 고지(THIRD_PARTY_NOTICES.md) -> npm 전문 -> Rust 전문(cargo-about).
// 날짜를 넣지 않아 같은 잠금 파일이면 결과가 바이트 단위로 같다.
// 사전 준비: cargo install cargo-about --locked --version 0.9.2 --features cli
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectNpmLicenses, renderNpmMarkdown } from './collect-npm-licenses.mjs';
import { CARGO_LOCK, NPM_LOCK, OUTPUT, headerLine, lockHash } from './license-hashes.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

// 코드 울타리 밖의 제목 수준을 한 단계 낮춘다.
function demoteHeadings(md) {
  let fence = null; // 열린 울타리 표시(``` 또는 ~~~~). 같은 표시로 닫힐 때까지 안쪽은 건드리지 않는다.
  return md
    .split('\n')
    .map((line) => {
      const m = /^(`{3,}|~{3,})/.exec(line);
      if (m) {
        if (!fence) fence = m[1];
        else if (m[1][0] === fence[0] && m[1].length >= fence.length && line.trim() === m[1]) fence = null;
        return line;
      }
      return !fence && /^#{1,5} /.test(line) ? `#${line}` : line;
    })
    .join('\n');
}

export function assemble({ notices, npmMd, rustMd, cargoHash, npmHash }) {
  const lines = [
    '# 전체 오픈소스 라이선스',
    '',
    '이 파일은 바슬즐 탐험대에 포함되거나 함께 쓰이는 제3자 구성 요소의 라이선스 전문입니다. 자동으로 만들어지며 직접 고치지 않습니다(`npm run licenses:full`).',
    '',
    '소스 코드 안내: 아래 Rust 크레이트는 수정하지 않고 사용했으며, 각 크레이트의 소스 코드는 아래에 적힌 이름과 버전으로 https://crates.io 에서 받을 수 있습니다. Mozilla Public License 2.0(MPL-2.0) 구성 요소도 같은 곳에서 원본 소스를 받을 수 있습니다.',
    '',
    headerLine(CARGO_LOCK.split('/').pop(), cargoHash),
    headerLine(NPM_LOCK, npmHash),
    '',
    '---',
    '',
    demoteHeadings(notices.replace(/\r\n/g, '\n').trimEnd()),
    '',
    '---',
    '',
    '## 화면(프런트엔드) npm 패키지 라이선스',
    '',
    demoteHeadings(npmMd.trimEnd()),
    '',
    '---',
    '',
    '## 프로그램(Rust) 크레이트 라이선스',
    '',
    '대상: x86_64-pc-windows-msvc, 실행 파일에 들어가는 의존성(빌드·개발 전용 제외). 두 가지 라이선스 중 고를 수 있는 크레이트는 한 가지로 묶어 표시합니다.',
    '',
    demoteHeadings(rustMd.trimEnd()),
    '',
  ];
  return lines.join('\n');
}

function runCargoAbout() {
  const dir = join(root, 'src-tauri');
  try {
    return execFileSync('cargo', ['about', 'generate', '--offline', '-c', 'about.toml', 'about.hbs'], {
      cwd: dir,
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (e) {
    throw new Error(
      `cargo-about 실행 실패. 설치: cargo install cargo-about --locked --version 0.9.2 --features cli\n${String(e.stderr ?? e.message).slice(-1500)}`,
    );
  }
}

function main() {
  const npmList = collectNpmLicenses(root);
  const out = assemble({
    notices: readFileSync(join(root, 'src-tauri/resources/THIRD_PARTY_NOTICES.md'), 'utf8'),
    npmMd: renderNpmMarkdown(npmList),
    rustMd: runCargoAbout(),
    cargoHash: lockHash(root, CARGO_LOCK),
    npmHash: lockHash(root, NPM_LOCK),
  });
  writeFileSync(join(root, OUTPUT), out);
  console.log(`${OUTPUT}: ${out.length}자, npm ${npmList.length}개`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
