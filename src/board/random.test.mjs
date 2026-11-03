// 실행: npm test  (Node 22의 타입 제거 실행으로 src/board/random.ts를 그대로 불러온다)
import test from 'node:test';
import assert from 'node:assert/strict';
import { seededRng, shuffle, present, splitTeams, pickWeighted, filterPlays, pickPlay, formatClock, RECENT_WEIGHT } from './random.ts';

const ids = (n) => Array.from({ length: n }, (_, i) => `s${i + 1}`);

test('같은 시드면 섞기 결과가 같고 원본은 그대로다', () => {
  const src = ids(10);
  const a = shuffle(src, seededRng(7));
  const b = shuffle(src, seededRng(7));
  assert.deepEqual(a, b);
  assert.deepEqual(src, ids(10));
  assert.deepEqual([...a].sort(), [...src].sort());
  assert.notDeepEqual(a, shuffle(src, seededRng(8)));
});

test('팀 나누기: 인원 차이 최대 1, 모두 한 번씩 배정', () => {
  for (let people = 1; people <= 31; people++) {
    for (let teams = 2; teams <= 6; teams++) {
      const out = splitTeams(ids(people), teams, seededRng(people * 10 + teams));
      const sizes = out.map((t) => t.length);
      assert.ok(Math.max(...sizes) - Math.min(...sizes) <= 1, `${people}명 ${teams}팀: ${sizes}`);
      assert.deepEqual(out.flat().sort(), ids(people).sort());
    }
  }
});

test('팀 나누기: 사람이 팀보다 적으면 팀 수를 줄인다', () => {
  const out = splitTeams(ids(3), 6, seededRng(1));
  assert.equal(out.length, 3);
  assert.ok(out.every((t) => t.length === 1));
  assert.deepEqual(splitTeams([], 3, seededRng(1)), [[]]);
});

test('빠진 학생은 팀에 들어가지 않는다', () => {
  const students = ids(12).map((id) => ({ id }));
  const absent = ['s2', 's5', 's9'];
  const here = present(students, absent);
  assert.equal(here.length, 9);
  const teams = splitTeams(here.map((s) => s.id), 3, seededRng(3));
  const flat = teams.flat();
  assert.equal(flat.length, 9);
  for (const a of absent) assert.ok(!flat.includes(a));
});

test('술래 뽑기: 중복 없이 요청한 수만큼, 후보 안에서만', () => {
  const pool = ids(8);
  for (let seed = 0; seed < 50; seed++) {
    const got = pickWeighted(pool, 5, [], seededRng(seed));
    assert.equal(got.length, 5);
    assert.equal(new Set(got).size, 5);
    for (const g of got) assert.ok(pool.includes(g));
  }
  assert.equal(pickWeighted(pool, 20, [], seededRng(1)).length, 8);
  assert.deepEqual(pickWeighted([], 2, [], seededRng(1)), []);
});

test('술래 뽑기: 최근 3번 안에 뽑힌 학생은 덜 뽑힌다', () => {
  const pool = ids(10);
  const history = [['s1'], ['s2'], ['s3']];
  const rng = seededRng(42);
  const counts = Object.fromEntries(pool.map((id) => [id, 0]));
  const trials = 20000;
  for (let i = 0; i < trials; i++) counts[pickWeighted(pool, 1, history, rng)[0]]++;
  const recentAvg = (counts.s1 + counts.s2 + counts.s3) / 3;
  const otherAvg = pool.slice(3).reduce((s, id) => s + counts[id], 0) / 7;
  assert.ok(recentAvg < otherAvg * 0.35, `최근 ${recentAvg}, 나머지 ${otherAvg}`);
  // 기대 비율: 0.15 / (3*0.15 + 7)
  const expected = (RECENT_WEIGHT / (3 * RECENT_WEIGHT + 7)) * trials;
  assert.ok(Math.abs(counts.s1 - expected) < expected * 0.35);
  assert.ok(recentAvg > 0, '완전히 빠지지는 않는다');
});

test('술래 뽑기: 4번 전에 뽑힌 학생은 다시 보통 확률', () => {
  const pool = ids(5);
  const history = [['s1'], ['s2'], ['s3'], ['s4']];
  const rng = seededRng(5);
  let s1 = 0;
  for (let i = 0; i < 5000; i++) if (pickWeighted(pool, 1, history, rng)[0] === 's1') s1++;
  assert.ok(s1 > 5000 / 5 * 0.8, `s1=${s1}`);
});

test('놀이 거르기: 장소·주제책·15분 이하', () => {
  const plays = [
    { id: 'a', place: 'outdoor', minutes: 10, topicBookIds: ['g1s1-korea'] },
    { id: 'b', place: 'indoor', minutes: 5, topicBookIds: [] },
    { id: 'c', place: 'both', minutes: 20, topicBookIds: ['g1s1-korea'] },
    { id: 'd', place: 'both', minutes: 15, topicBookIds: ['g2s1-me'] },
  ];
  const f = { place: 'all', topicBookId: null, onlyTopic: false, short: false };
  const idsOf = (x) => x.map((p) => p.id).join('');
  assert.equal(idsOf(filterPlays(plays, f)), 'abcd');
  assert.equal(idsOf(filterPlays(plays, { ...f, place: 'outdoor' })), 'acd');
  assert.equal(idsOf(filterPlays(plays, { ...f, place: 'indoor' })), 'bcd');
  assert.equal(idsOf(filterPlays(plays, { ...f, short: true })), 'abd');
  assert.equal(idsOf(filterPlays(plays, { ...f, onlyTopic: true, topicBookId: 'g1s1-korea' })), 'ac');
  assert.equal(idsOf(filterPlays(plays, { ...f, onlyTopic: true, topicBookId: null })), 'abcd');
});

test('놀이 룰렛: 같은 놀이가 연속으로 나오지 않는다', () => {
  const plays = [{ id: 'a' }, { id: 'b' }, { id: 'c' }];
  const rng = seededRng(9);
  let last = null;
  for (let i = 0; i < 500; i++) {
    const p = pickPlay(plays, last, rng);
    assert.notEqual(p.id, last);
    last = p.id;
  }
  assert.equal(pickPlay([{ id: 'x' }], 'x', rng).id, 'x');
  assert.equal(pickPlay([], null, rng), null);
});

test('시계 표기', () => {
  assert.equal(formatClock(0), '0:00');
  assert.equal(formatClock(59.2), '1:00');
  assert.equal(formatClock(61), '1:01');
  assert.equal(formatClock(600), '10:00');
  assert.equal(formatClock(-3), '0:00');
});
