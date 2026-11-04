// 라이선스 파일이 최신인지 확인하는 데 쓰는 잠금 파일 해시. 줄바꿈 차이(CRLF)는 무시한다.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

export const CARGO_LOCK = 'src-tauri/Cargo.lock';
export const NPM_LOCK = 'package-lock.json';
export const OUTPUT = 'src-tauri/resources/THIRD_PARTY_LICENSES.md';

export function lockHash(rootDir, rel) {
  const text = readFileSync(join(rootDir, rel), 'utf8').replace(/\r\n/g, '\n');
  return createHash('sha256').update(text).digest('hex');
}

export function headerLine(label, hash) {
  return `- ${label} SHA-256: \`${hash}\``;
}

export function parseHeaderHash(markdown, label) {
  const m = new RegExp(`^- ${label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} SHA-256: \`([0-9a-f]{64})\``, 'm').exec(markdown);
  return m ? m[1] : null;
}
