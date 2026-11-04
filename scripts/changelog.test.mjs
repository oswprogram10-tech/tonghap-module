import { test } from 'node:test';
import assert from 'node:assert/strict';
import { extractSection } from './changelog.mjs';

const sample = `# 변경 내용

## [0.2.0] - 2026-11-05
- 새 기능
- 고침

## [0.1.0] - 2026-11-01
- 첫 버전
`;

test('중간 구역은 다음 제목 전까지만 뽑는다', () => {
  assert.equal(extractSection(sample, '0.2.0'), '- 새 기능\n- 고침');
});

test('마지막 구역은 끝까지 뽑는다', () => {
  assert.equal(extractSection(sample, '0.1.0'), '- 첫 버전');
});

test('없는 버전은 null', () => {
  assert.equal(extractSection(sample, '0.3.0'), null);
});

test('버전 접두사가 같은 다른 구역과 섞이지 않는다', () => {
  const t = '## [0.12.0] - d\n- a\n\n## [0.1.0] - d\n- b\n';
  assert.equal(extractSection(t, '0.1.0'), '- b');
  assert.equal(extractSection(t, '0.12'), null);
});

test('내용이 비어 있으면 null, CRLF도 처리', () => {
  assert.equal(extractSection('## [1.0.0] - d\n\n## [0.9.0] - d\n- x\n', '1.0.0'), null);
  assert.equal(extractSection('## [1.0.0] - d\r\n- x\r\n', '1.0.0'), '- x');
});
