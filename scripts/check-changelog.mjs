// package.json의 현재 버전에 해당하는 CHANGELOG.md 구역이 있는지 확인한다.
import { readFileSync } from 'node:fs';
import { extractSection } from './changelog.mjs';

const { version } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const text = readFileSync(new URL('../CHANGELOG.md', import.meta.url), 'utf8');
const body = extractSection(text, version);
if (!body) {
  console.error(`CHANGELOG.md에 [${version}] 구역이 없거나 비어 있습니다. 버전을 올릴 때 변경 내용을 함께 적어 주세요.`);
  process.exit(1);
}
const dated = new RegExp(`^## \\[${version.replace(/\./g, '\\.')}\\] - \\d{4}-\\d{2}-\\d{2}\\s*$`, 'm');
if (!dated.test(text)) {
  console.error(`CHANGELOG.md의 [${version}] 제목은 "## [${version}] - YYYY-MM-DD" 형식이어야 합니다.`);
  process.exit(1);
}
console.log(`CHANGELOG.md [${version}] 구역 확인`);
