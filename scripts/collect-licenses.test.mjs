import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const file = fileURLToPath(new URL('../src/about/licenses.json', import.meta.url));
const data = JSON.parse(readFileSync(file, 'utf8'));
const find = (list, name) => list.find((x) => x.name === name);

test('npm 직접 의존성에 이름·버전·라이선스가 있다', () => {
  for (const name of ['react', '@tauri-apps/api']) {
    const e = find(data.npm, name);
    assert.ok(e, `${name} 없음`);
    assert.match(e.version, /^\d+\.\d+\.\d+/);
    assert.ok(e.license && e.license !== '확인 필요');
  }
});

test('Rust 직접 의존성에 tauri와 rusqlite가 있다', () => {
  for (const name of ['tauri', 'rusqlite']) {
    const e = find(data.rust, name);
    assert.ok(e, `${name} 없음`);
    assert.match(e.version, /^\d+\.\d+\.\d+/);
    assert.ok(e.license);
  }
});

test('이름순으로 정렬되어 있다', () => {
  for (const list of [data.npm, data.rust]) {
    const names = list.map((x) => x.name);
    assert.deepEqual(names, [...names].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)));
  }
});
