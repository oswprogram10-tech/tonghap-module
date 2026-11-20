// 실행: npm test
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  WEAK_NEGATIVE,
  WEAK_NONE,
  WEAK_SESSION_ONLY,
  afterReview,
  confirmLabel,
  dotDate,
  firstUnreviewed,
  neisBytes,
  nextUnconfirmed,
  startStep,
  weakReason,
} from './stepLogic.ts';

const obs = (id, negative = false) => ({ kind: 'observation', id, date: '2026-09-03', topic: null, standards: [], tags: [], text: '관찰', together: false, negative });
const ses = (id) => ({ kind: 'session', id, date: '2026-11-10', topic: null, title: '차시', standards: [] });
const hab = (id) => ({ kind: 'habit', id, title: '습관', doneDays: 3, schoolDays: 5 });
const map = (...list) => new Map(list.map((e) => [e.id, e]));
const rv = (...flags) => flags.map((reviewed) => ({ text: 'x', evidence: [], reviewed }));

test('약한 근거: 근거가 없으면 붙이거나 빼라고 안내한다', () => {
  assert.deepEqual(weakReason([], map(obs('a'))), { kind: 'none', message: WEAK_NONE });
  assert.equal(weakReason(['없는id'], map(obs('a'))).kind, 'none');
});

test('약한 근거: 차시·습관뿐이고 관찰이 없으면 안내한다', () => {
  assert.deepEqual(weakReason(['s'], map(ses('s'))), { kind: 'sessionOnly', message: WEAK_SESSION_ONLY });
  assert.equal(weakReason(['s', 'h'], map(ses('s'), hab('h'))).kind, 'sessionOnly');
});

test('약한 근거: 관찰이 하나라도 있으면 약하지 않다', () => {
  assert.equal(weakReason(['s', 'a'], map(ses('s'), obs('a'))), null);
});

test('약한 근거: 지도가 필요한 모습 근거는 변화 가능성을 확인하라고 안내한다', () => {
  assert.deepEqual(weakReason(['n'], map(obs('n', true))), { kind: 'negative', message: WEAK_NEGATIVE });
  assert.equal(weakReason(['a', 'n'], map(obs('a'), obs('n', true))).kind, 'negative');
});

test('다음 미확정 학생: 지금 다음 순서부터 한 바퀴 돈다', () => {
  const items = [
    { id: 'a', done: false },
    { id: 'b', done: true },
    { id: 'c', done: false },
    { id: 'd', done: true },
  ];
  assert.equal(nextUnconfirmed(items, 'a'), 'c');
  assert.equal(nextUnconfirmed(items, 'c'), 'a');
  assert.equal(nextUnconfirmed(items, 'd'), 'a');
  assert.equal(nextUnconfirmed(items, 'b'), 'c');
});

test('다음 미확정 학생: 남은 학생이 없으면 null, 지금 학생만 남아도 null', () => {
  assert.equal(nextUnconfirmed([{ id: 'a', done: true }], 'a'), null);
  assert.equal(nextUnconfirmed([{ id: 'a', done: false }], 'a'), null);
  assert.equal(nextUnconfirmed([], null), null);
  assert.equal(nextUnconfirmed([{ id: 'a', done: false }], null), 'a');
});

test('시작 단계: 초안 없음 → 1, 확정 → 4, 미확정 → 3의 첫 미확인 문장', () => {
  assert.deepEqual(startStep('none', []), { step: 1, index: 0 });
  assert.deepEqual(startStep('draft', []), { step: 1, index: 0 });
  assert.deepEqual(startStep('confirmed', rv(true, true)), { step: 4, index: 0 });
  assert.deepEqual(startStep('draft', rv(true, false, false)), { step: 3, index: 1 });
  assert.deepEqual(startStep('draft', rv(false, true)), { step: 3, index: 0 });
});

test('시작 단계: 모두 확인했지만 아직 확정 전이면 4', () => {
  assert.deepEqual(startStep('draft', rv(true, true)), { step: 4, index: 0 });
});

test('첫 미확인 문장', () => {
  assert.equal(firstUnreviewed(rv(true, true)), null);
  assert.equal(firstUnreviewed(rv(true, false)), 1);
});

test('확인한 뒤: 다음 문장 → 남은 미확인 문장 → 확정 단계', () => {
  assert.deepEqual(afterReview(rv(false, false, false), 0), { step: 3, index: 1 });
  assert.deepEqual(afterReview(rv(false, true, false), 2), { step: 3, index: 0 });
  assert.deepEqual(afterReview(rv(true, true, false), 2), { step: 4, index: 2 });
  assert.deepEqual(afterReview(rv(false), 0), { step: 4, index: 0 });
});

test('확정 단추 문구', () => {
  assert.deepEqual(confirmLabel({ count: 0, firstUnreviewed: null, canConfirm: false }), { label: '확정', action: 'none', index: 0 });
  assert.deepEqual(confirmLabel({ count: 4, firstUnreviewed: 2, canConfirm: true }), { label: '3번 문장 확인 필요', action: 'goto', index: 2 });
  assert.deepEqual(confirmLabel({ count: 4, firstUnreviewed: null, canConfirm: true }), { label: '확정', action: 'confirm', index: 0 });
  assert.deepEqual(confirmLabel({ count: 4, firstUnreviewed: null, canConfirm: false }), { label: '확정', action: 'blocked', index: 0 });
  assert.equal(confirmLabel({ count: 1, firstUnreviewed: 0, canConfirm: true, unit: '통합본' }).label, '통합본 확인 필요');
});

test('NEIS 바이트: 한글 3, 영문·숫자 1, 줄바꿈 1, \\r은 0', () => {
  assert.equal(neisBytes('가나다'), 9);
  assert.equal(neisBytes('abc 12'), 6);
  assert.equal(neisBytes('가\r\n나'), 3 + 1 + 3);
  assert.equal(neisBytes('「」'), 6);
});

test('날짜 표시', () => {
  assert.equal(dotDate('2026-08-01'), '2026.8.1');
  assert.equal(dotDate('2027-02-28'), '2027.2.28');
  assert.equal(dotDate(''), '');
});
