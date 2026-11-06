// 실행: npm test  (타이머 안내 음성 시점 계산)
import test from 'node:test';
import assert from 'node:assert/strict';
import { dueCue, markPassed, parseSoundMode, trimWav } from './cues.ts';
import { encodeWav16 } from '../stt/wav.ts';

/** 1초씩 흘려 보내며(처음 시작 시점 포함) 나온 안내를 모은다. */
function run(total, { from = total, to = 0, played = new Set(), start = true } = {}) {
  const out = [];
  let p = start ? markPassed(from, total, played) : played;
  for (let t = from; t >= to; t -= 1) {
    const r = dueCue(t, total, p);
    p = r.played;
    if (r.cue) out.push([t, r.cue]);
  }
  return { out, played: p };
}

test('전체 시간별로 나오는 안내가 다르다', () => {
  assert.deepEqual(run(30).out, []);
  assert.deepEqual(run(59).out, []);
  assert.deepEqual(run(60).out, [[30, 'timer-30s']]);
  assert.deepEqual(run(90).out, [[30, 'timer-30s']]);
  assert.deepEqual(run(119).out, [[30, 'timer-30s']]);
  assert.deepEqual(run(120).out, [[60, 'timer-1min'], [30, 'timer-30s']]);
  assert.deepEqual(run(300).out, [[60, 'timer-1min'], [30, 'timer-30s']]);
});

test('시점 직전에는 아직 나오지 않고 시점에 닿으면 나온다', () => {
  assert.equal(dueCue(60.2, 300, new Set()).cue, null);
  assert.equal(dueCue(60, 300, new Set()).cue, 'timer-1min');
  assert.equal(dueCue(30.01, 300, new Set(['timer-1min'])).cue, null);
  assert.equal(dueCue(29.9, 300, new Set(['timer-1min'])).cue, 'timer-30s');
});

test('같은 안내는 두 번 나오지 않는다', () => {
  const a = dueCue(55, 300, new Set());
  assert.equal(a.cue, 'timer-1min');
  assert.equal(dueCue(54.8, 300, a.played).cue, null);
});

test('일시정지 후 다시 시작해도 이미 낸 안내는 반복하지 않는다', () => {
  // 40초 남기고 멈춘 뒤 다시 시작: 1분 안내는 이미 나왔다
  const first = run(300, { to: 40 });
  assert.deepEqual(first.out, [[60, 'timer-1min']]);
  const second = run(300, { from: 40, played: first.played });
  assert.deepEqual(second.out, [[30, 'timer-30s']]);
  // 25초에서 멈췄다 다시 시작: 30초 안내는 한 번뿐
  const a = run(300, { to: 25 });
  const b = run(300, { from: 25, played: a.played });
  assert.deepEqual(b.out, []);
  assert.equal([...a.out, ...b.out].filter(([, c]) => c === 'timer-30s').length, 1);
});

test('아직 지나지 않은 시점에서 멈췄다 다시 시작하면 그 안내가 나온다', () => {
  const a = run(300, { to: 70 });
  assert.deepEqual(a.out, []);
  const b = run(300, { from: 70, played: a.played });
  assert.deepEqual(b.out, [[60, 'timer-1min'], [30, 'timer-30s']]);
});

test('이미 지난 시점에서 시작하면 소리 없이 건너뛴다', () => {
  assert.deepEqual([...markPassed(20, 300, new Set())].sort(), ['timer-1min', 'timer-30s']);
  assert.deepEqual([...markPassed(45, 300, new Set())], ['timer-1min']);
  assert.deepEqual([...markPassed(300, 300, new Set())], []);
  assert.deepEqual([...markPassed(20, 45, new Set())], []);
  const r = run(300, { from: 20 });
  assert.deepEqual(r.out, []);
});

test('시간을 바꾸면 새로 센다(안내 기록을 비운 상태에서 시작)', () => {
  const a = run(300);
  assert.equal(a.out.length, 2);
  const b = run(120); // setTotal은 기록을 비운다
  assert.equal(b.out.length, 2);
});

test('탭이 멈춰 있다 돌아와 여러 시점을 한꺼번에 지나면 가장 가까운 안내 하나만 낸다', () => {
  const r = dueCue(20, 300, new Set());
  assert.equal(r.cue, 'timer-30s');
  assert.deepEqual([...r.played].sort(), ['timer-1min', 'timer-30s']);
});

test('소리 방식 저장값: 예전 off는 끔, 값이 없거나 on이면 안내 음성', () => {
  assert.equal(parseSoundMode('off'), 'off');
  assert.equal(parseSoundMode('beep'), 'beep');
  assert.equal(parseSoundMode('voice'), 'voice');
  assert.equal(parseSoundMode('on'), 'voice');
  assert.equal(parseSoundMode(null), 'voice');
  assert.equal(parseSoundMode('이상한 값'), 'voice');
});

test('녹음 WAV를 10초로 자른다', () => {
  const rate = 16000;
  const long = encodeWav16(new Float32Array(rate * 12).fill(0.1), rate);
  const cut = trimWav(long, 10);
  assert.equal(cut.length, 44 + rate * 10 * 2);
  const v = new DataView(cut.buffer);
  assert.equal(v.getUint32(40, true), rate * 10 * 2);
  assert.equal(v.getUint32(4, true), 36 + rate * 10 * 2);
  const short = encodeWav16(new Float32Array(rate * 3), rate);
  assert.equal(trimWav(short, 10), short);
});
