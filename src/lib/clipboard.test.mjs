import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { copyText } from './clipboard.ts';

const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function deps({ tauri = true, hooks = true, hooksFail = false } = {}) {
  const calls = { written: [], recorded: [] };
  return {
    calls,
    deps: {
      isTauri: () => tauri,
      debugHooks: async () => {
        if (hooksFail) throw new Error('명령 없음');
        return hooks;
      },
      write: async (t) => void calls.written.push(t),
      record: (t) => void calls.recorded.push(t),
    },
  };
}

test('디버그 빌드(hooks=true)에서는 복사한 글을 남기고 클립보드에도 쓴다', async () => {
  const d = deps();
  await copyText('안녕', d.deps);
  assert.deepEqual(d.calls, { written: ['안녕'], recorded: ['안녕'] });
});

test('릴리스 빌드(hooks=false)에서는 아무것도 남기지 않고 클립보드에만 쓴다', async () => {
  const d = deps({ hooks: false });
  await copyText('안녕', d.deps);
  assert.deepEqual(d.calls, { written: ['안녕'], recorded: [] });
});

test('명령이 실패하거나 Tauri가 아니면 남기지 않는다', async () => {
  for (const o of [{ hooksFail: true }, { tauri: false }]) {
    const d = deps(o);
    await copyText('글', d.deps);
    assert.deepEqual(d.calls, { written: ['글'], recorded: [] });
  }
});

function sources(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return sources(p);
    return /\.(ts|tsx)$/.test(e.name) ? [p] : [];
  });
}

test('navigator.clipboard.writeText 는 src/lib/clipboard.ts 에서만 부른다', () => {
  const wrapper = path.join(SRC, 'lib', 'clipboard.ts');
  const offenders = sources(SRC).filter((f) => f !== wrapper && /clipboard\.writeText/.test(fs.readFileSync(f, 'utf8')));
  assert.deepEqual(offenders.map((f) => path.relative(SRC, f)), []);
});
