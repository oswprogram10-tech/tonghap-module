import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { mediaUrlFor } from './media.ts';

const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ROOT = path.resolve(SRC, '..');

test('윈도우는 http://tgmedia.localhost/<id>, 그 밖은 tgmedia://localhost/<id>', () => {
  const id = '3f2b8c1e-aaaa-bbbb-cccc-123456789abc';
  assert.equal(mediaUrlFor(id, 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/130'), `http://tgmedia.localhost/${id}`);
  assert.equal(mediaUrlFor(id, 'Mozilla/5.0 (Linux; Android 14)'), `http://tgmedia.localhost/${id}`);
  assert.equal(mediaUrlFor(id, 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15'), `tgmedia://localhost/${id}`);
  assert.equal(mediaUrlFor(id, 'Mozilla/5.0 (X11; Linux x86_64)'), `tgmedia://localhost/${id}`);
});

test('id의 특수문자는 인코딩되어 경로를 벗어나지 않는다', () => {
  assert.equal(mediaUrlFor('../x y', 'X11'), 'tgmedia://localhost/..%2Fx%20y');
});

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(e.name)) out.push(p);
  }
  return out;
}

test('src 안에 convertFileSrc가 남지 않는다(파일 경로 대신 첨부 id를 쓴다)', () => {
  const hits = walk(SRC).filter((f) => fs.readFileSync(f, 'utf8').includes('convertFileSrc'));
  assert.deepEqual(hits.map((f) => path.relative(ROOT, f)), []);
});

test('설정에서 asset 프로토콜이 빠졌고 tgmedia만 허용한다', () => {
  const conf = JSON.parse(fs.readFileSync(path.join(ROOT, 'src-tauri/tauri.conf.json'), 'utf8'));
  const sec = conf.app.security;
  assert.equal(sec.assetProtocol, undefined);
  assert.ok(!/asset/.test(sec.csp), sec.csp);
  for (const dir of ['img-src', 'media-src', 'connect-src']) {
    const part = sec.csp.split(';').map((s) => s.trim()).find((s) => s.startsWith(dir));
    assert.ok(part && part.includes('tgmedia:') && part.includes('http://tgmedia.localhost'), `${dir}: ${part}`);
  }
  const cargo = fs.readFileSync(path.join(ROOT, 'src-tauri/Cargo.toml'), 'utf8');
  assert.ok(!cargo.includes('protocol-asset'));
});
