// CHANGELOG.md에서 한 버전의 구역을 뽑는다. check-changelog.mjs와 release.yml이 함께 쓴다.
// 쓰는 법: node scripts/changelog.mjs <버전> [출력 파일]  (없으면 화면에 출력, 구역이 없으면 오류로 끝남)
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/** `## [버전]`으로 시작하는 구역의 본문(제목 줄 제외, 앞뒤 공백 제거)을 돌려준다. 없거나 비어 있으면 null. */
export function extractSection(text, version) {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  const head = `## [${version}]`;
  const start = lines.findIndex((l) => l.trim() === head || l.startsWith(head + ' '));
  if (start < 0) return null;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^## /.test(lines[i])) {
      end = i;
      break;
    }
  }
  const body = lines.slice(start + 1, end).join('\n').trim();
  return body === '' ? null : body;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [version, out] = process.argv.slice(2);
  if (!version) {
    console.error('사용법: node scripts/changelog.mjs <버전> [출력 파일]');
    process.exit(2);
  }
  const text = readFileSync(new URL('../CHANGELOG.md', import.meta.url), 'utf8');
  const body = extractSection(text, version);
  if (!body) {
    console.error(`CHANGELOG.md에 [${version}] 구역이 없거나 비어 있습니다. 릴리스 전에 변경 내용을 적어 주세요.`);
    process.exit(1);
  }
  if (out) writeFileSync(out, body + '\n');
  else console.log(body);
}
