import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { collectNpmLicenses, renderNpmMarkdown } from './collect-npm-licenses.mjs';
import { CARGO_LOCK, NPM_LOCK, OUTPUT } from './license-hashes.mjs';
import { checkLicenses } from './validate-third-party-licenses.mjs';

const repo = fileURLToPath(new URL('..', import.meta.url));

test('npm 수집: react, react-dom 의 MIT 전문이 들어 있고 이름@버전은 중복이 없다', () => {
  const list = collectNpmLicenses(repo);
  for (const name of ['react', 'react-dom']) {
    const e = list.find((x) => x.name === name);
    assert.ok(e, `${name} 없음`);
    assert.ok(e.text?.includes('Permission is hereby granted'), `${name} 라이선스 전문 없음`);
  }
  const keys = list.map((x) => `${x.name}@${x.version}`);
  assert.equal(new Set(keys).size, keys.length);
  assert.ok(renderNpmMarkdown(list).includes('#### react '));
});

test('검증: 잠금 파일 해시가 바뀌면 오래된 것으로 알린다', () => {
  const dir = mkdtempSync(join(tmpdir(), 'lic-'));
  try {
    for (const rel of [CARGO_LOCK, NPM_LOCK, OUTPUT]) {
      mkdirSync(join(dir, rel, '..'), { recursive: true });
      copyFileSync(join(repo, rel), join(dir, rel));
    }
    assert.deepEqual(checkLicenses(dir), [], '복사본은 최신이어야 함');
    writeFileSync(join(dir, NPM_LOCK), readFileSync(join(dir, NPM_LOCK), 'utf8') + '\n ');
    const problems = checkLicenses(dir);
    assert.equal(problems.length, 1);
    assert.match(problems[0], /package-lock\.json/);
    writeFileSync(join(dir, CARGO_LOCK), 'changed');
    assert.equal(checkLicenses(dir).length, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
