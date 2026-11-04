import assert from 'node:assert/strict';
import { test } from 'node:test';
import { crc32, readZipEntries } from './zip.mjs';

/** 앱(support.rs zip_bytes)과 같은 모양의 압축 없는 zip을 만든다. */
function makeZip(entries, method = 0) {
  const parts = [];
  const central = [];
  let offset = 0;
  for (const e of entries) {
    const name = Buffer.from(e.name, 'utf8');
    const data = Buffer.from(e.data);
    const h = Buffer.alloc(30);
    h.writeUInt32LE(0x04034b50, 0);
    h.writeUInt16LE(20, 4);
    h.writeUInt16LE(0x0800, 6);
    h.writeUInt16LE(method, 8);
    h.writeUInt32LE(crc32(data), 14);
    h.writeUInt32LE(data.length, 18);
    h.writeUInt32LE(data.length, 22);
    h.writeUInt16LE(name.length, 26);
    parts.push(h, name, data);
    const c = Buffer.alloc(46);
    c.writeUInt32LE(0x02014b50, 0);
    c.writeUInt16LE(20, 4);
    c.writeUInt16LE(20, 6);
    c.writeUInt16LE(0x0800, 8);
    c.writeUInt16LE(method, 10);
    c.writeUInt32LE(crc32(data), 16);
    c.writeUInt32LE(data.length, 20);
    c.writeUInt32LE(data.length, 24);
    c.writeUInt16LE(name.length, 28);
    c.writeUInt32LE(offset, 42);
    central.push(c, name);
    offset += 30 + name.length + data.length;
  }
  const cd = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, cd, end]);
}

test('crc32 알려진 값', () => {
  assert.equal(crc32(Buffer.from('123456789')), 0xcbf43926);
});

test('이름 목록과 내용을 읽는다(한글 이름 포함)', () => {
  const zip = makeZip([
    { name: 'about.json', data: '{"appVersion":"1.2.3"}' },
    { name: 'logs/app.log', data: '시작: 앱\n' },
    { name: 'diag/기기-점검.json', data: '' },
  ]);
  const got = readZipEntries(zip);
  assert.deepEqual(got.map((e) => e.name), ['about.json', 'logs/app.log', 'diag/기기-점검.json']);
  assert.equal(got[1].data.toString('utf8'), '시작: 앱\n');
  assert.equal(got[2].data.length, 0);
});

test('zip이 아니거나 압축·CRC가 틀리면 한국어 메시지로 던진다', () => {
  assert.throws(() => readZipEntries(Buffer.from('아무 글')), /zip 파일이 아닙니다/);
  assert.throws(() => readZipEntries(makeZip([{ name: 'a', data: 'x' }], 8)), /압축된 항목/);
  const bad = makeZip([{ name: 'a', data: 'xyz' }]);
  bad[bad.indexOf('xyz')] = 0x41;
  assert.throws(() => readZipEntries(bad), /CRC/);
});
