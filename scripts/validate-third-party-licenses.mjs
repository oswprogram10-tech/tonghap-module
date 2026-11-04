// THIRD_PARTY_LICENSES.md 머리말의 잠금 파일 해시가 현재 파일과 같은지 확인한다.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CARGO_LOCK, NPM_LOCK, OUTPUT, lockHash, parseHeaderHash } from './license-hashes.mjs';

const defaultRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

// 문제 목록을 돌려준다(비어 있으면 최신).
export function checkLicenses(rootDir = defaultRoot) {
  const file = join(rootDir, OUTPUT);
  if (!existsSync(file)) return [`${OUTPUT} 파일이 없습니다.`];
  const md = readFileSync(file, 'utf8');
  const problems = [];
  for (const [label, rel] of [
    [CARGO_LOCK.split('/').pop(), CARGO_LOCK],
    [NPM_LOCK, NPM_LOCK],
  ]) {
    const recorded = parseHeaderHash(md, label);
    if (!recorded) problems.push(`${OUTPUT} 머리말에서 ${label} 해시를 찾을 수 없습니다.`);
    else if (recorded !== lockHash(rootDir, rel)) problems.push(`${rel} 가 바뀌었는데 ${OUTPUT} 는 예전 그대로입니다.`);
  }
  return problems;
}

function main() {
  const problems = checkLicenses();
  if (problems.length === 0) {
    console.log('THIRD_PARTY_LICENSES.md 가 최신입니다.');
    return;
  }
  for (const p of problems) console.error(`오류: ${p}`);
  console.error('의존성이 바뀌었습니다. `npm run licenses:full` 을 실행해 라이선스 파일을 다시 만들고 함께 커밋하세요.');
  process.exit(1);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
