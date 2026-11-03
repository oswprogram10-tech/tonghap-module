// 실행: npm test
import test from 'node:test';
import assert from 'node:assert/strict';
import { encodeWav16 } from './wav.ts';

const view = (b) => new DataView(b.buffer, b.byteOffset, b.byteLength);
const text = (b, at, n) => String.fromCharCode(...b.slice(at, at + n));

test('WAV 헤더: RIFF 크기, fmt 청크, 샘플레이트, 비트 수, data 크기', () => {
  const wav = encodeWav16(new Float32Array(160), 16000);
  const v = view(wav);
  assert.equal(wav.length, 44 + 320);
  assert.equal(text(wav, 0, 4), 'RIFF');
  assert.equal(v.getUint32(4, true), 36 + 320);
  assert.equal(text(wav, 8, 8), 'WAVEfmt ');
  assert.equal(v.getUint32(16, true), 16);
  assert.equal(v.getUint16(20, true), 1); // PCM
  assert.equal(v.getUint16(22, true), 1); // 모노
  assert.equal(v.getUint32(24, true), 16000);
  assert.equal(v.getUint32(28, true), 32000);
  assert.equal(v.getUint16(32, true), 2);
  assert.equal(v.getUint16(34, true), 16);
  assert.equal(text(wav, 36, 4), 'data');
  assert.equal(v.getUint32(40, true), 320);
});

test('샘플 변환과 ±1 초과 값 자르기', () => {
  const wav = encodeWav16(new Float32Array([0, 1, -1, 2.5, -3, 0.5, NaN]), 16000);
  const v = view(wav);
  const s = (i) => v.getInt16(44 + i * 2, true);
  assert.deepEqual([s(0), s(1), s(2), s(3), s(4), s(6)], [0, 32767, -32768, 32767, -32768, 0]);
  assert.equal(s(5), 16384);
});

test('길이 0이면 헤더만 있는 올바른 WAV', () => {
  const wav = encodeWav16(new Float32Array(0), 16000);
  const v = view(wav);
  assert.equal(wav.length, 44);
  assert.equal(v.getUint32(4, true), 36);
  assert.equal(v.getUint32(40, true), 0);
});
