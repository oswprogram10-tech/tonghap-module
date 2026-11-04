import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WRAPPER = path.join(SRC, 'lib', 'dialogs.ts');

function sources(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return sources(p);
    return /\.(ts|tsx)$/.test(e.name) ? [p] : [];
  });
}

test('대화상자 플러그인은 src/lib/dialogs.ts 에서만 가져온다', () => {
  const offenders = sources(SRC).filter((f) => f !== WRAPPER && /plugin-dialog/.test(fs.readFileSync(f, 'utf8')));
  assert.deepEqual(offenders.map((f) => path.relative(SRC, f)), [], '화면 코드는 ../lib/dialogs 를 거쳐야 합니다(E2E 대화상자 대체가 동작하려면 필요)');
  assert.match(fs.readFileSync(WRAPPER, 'utf8'), /@tauri-apps\/plugin-dialog/);
});

test('래퍼는 진짜 대화상자보다 먼저 e2e_dialog_next 를 묻는다', () => {
  const text = fs.readFileSync(WRAPPER, 'utf8');
  for (const fn of ['pickFiles', 'pickFile', 'pickFolder', 'saveFile']) {
    const start = text.indexOf(`export async function ${fn}(`);
    assert.notEqual(start, -1, fn);
    const next = text.indexOf('export ', start + 1);
    const body = text.slice(start, next === -1 ? undefined : next);
    const fake = body.indexOf('await scripted(');
    const real = body.search(/await (open|save)\(/);
    assert.ok(fake !== -1 && real !== -1 && fake < real, `${fn}: 대체 확인이 진짜 대화상자보다 앞서야 합니다`);
  }
  assert.match(text, /e2e_dialog_next/);
});
