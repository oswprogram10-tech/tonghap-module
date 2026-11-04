import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { generate } from './generate-bundles.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../..');
const phrasesPath = resolve(here, 'fixtures/phrases.fixture.json');
const templates = JSON.parse(readFileSync(resolve(root, 'data/evaluation/templates.json'), 'utf8'));
const curriculum = JSON.parse(readFileSync(resolve(root, 'data/curriculum/integrated-2022.json'), 'utf8'));
const run = (students, seed) => generate({ students, seed, phrasesPath });
const rows = (text) => text.trim().split('\n').map((l) => JSON.parse(l));

test('같은 시드는 바이트까지 같은 결과, 다른 시드는 다른 결과', () => {
  assert.equal(run(20, 7), run(20, 7));
  assert.notEqual(run(20, 7), run(20, 8));
});

test('학생 수 × 교과 3개의 묶음, 설계 08 형식', () => {
  const rs = rows(run(15, 1));
  assert.equal(rs.length, 45);
  const idRe = /^[osh]:[0-9a-f]{12}$/;
  for (const { meta, bundle } of rs) {
    assert.deepEqual(Object.keys(meta), ['syntheticStudent', 'subject', 'seed']);
    assert.equal(meta.subject, bundle.subject);
    assert.match(bundle.period.from, /^2027-\d\d-\d\d$/);
    assert.ok(bundle.period.from < bundle.period.to);
    for (const e of bundle.evidence) {
      assert.match(e.id, idRe);
      assert.equal(e.id[0], { observation: 'o', session: 's', habit: 'h' }[e.kind]);
      if (e.kind === 'habit') {
        assert.equal(bundle.subject, '바');
        assert.ok(Number.isInteger(e.doneDays) && e.doneDays >= 0 && e.doneDays <= e.schoolDays && e.schoolDays > 0);
      } else {
        assert.ok(e.date >= bundle.period.from && e.date <= bundle.period.to, `기간 밖 날짜 ${e.date}`);
        assert.ok(e.topic && Array.isArray(e.standards));
        for (const c of e.standards) assert.ok(c in bundle.standardsText, `standardsText에 ${c} 없음`);
      }
      if (e.kind === 'observation') {
        assert.ok(e.text.length > 0);
        assert.equal(typeof e.together, 'boolean');
        assert.ok(e.tags.length >= 1);
      }
    }
    for (const code of Object.keys(bundle.standardsText)) {
      const s = curriculum.standards.find((x) => x.code === code);
      assert.equal(bundle.standardsText[code], s.statement);
      assert.equal(s.subject, bundle.subject);
    }
  }
});

test('묶음 안과 전체에서 근거 id가 겹치지 않고, 근거는 3~12개', () => {
  const all = new Set();
  for (const { bundle } of rows(run(60, 3))) {
    assert.ok(bundle.evidence.length >= 3 && bundle.evidence.length <= 12, `근거 ${bundle.evidence.length}개`);
    for (const e of bundle.evidence) {
      assert.ok(!all.has(e.id), `id 중복 ${e.id}`);
      all.add(e.id);
    }
  }
});

test('학생 이름 없이 alias는 학생 하나뿐', () => {
  for (const { bundle } of rows(run(20, 5))) {
    assert.deepEqual(bundle.student, { alias: '학생' });
  }
});

test('태그가 교과에 맞고, 인용·함께·습관이 섞임', () => {
  let quotes = 0, noQuotes = 0, together = 0, habits = 0, bareun = 0;
  const lo = { min: 1, max: 0 };
  for (const { bundle } of rows(run(80, 2))) {
    const allowed = Object.keys(templates.tagGroups[bundle.subject]);
    if (bundle.subject === '바') {
      bareun++;
      const h = bundle.evidence.filter((e) => e.kind === 'habit');
      assert.ok(h.length >= 1, '바 묶음에 습관 근거 필요');
      habits += h.length;
      for (const x of h) { const r = x.doneDays / x.schoolDays; lo.min = Math.min(lo.min, r); lo.max = Math.max(lo.max, r); }
    } else assert.ok(!bundle.evidence.some((e) => e.kind === 'habit'));
    for (const e of bundle.evidence.filter((x) => x.kind === 'observation')) {
      for (const t of e.tags) assert.ok(allowed.includes(t), `${bundle.subject}에 맞지 않는 태그 ${t}`);
      if (e.quote) quotes++; else noQuotes++;
      if (e.together) together++;
    }
  }
  assert.ok(quotes > 0 && noQuotes > 0 && together > 0 && habits >= bareun);
  assert.ok(lo.max - lo.min > 0.4, '습관 실천율이 고르게 퍼져야 함');
});

test('주제책은 그 학년·학기 것이고 날짜가 학기 안에 퍼짐', () => {
  const books = new Map(curriculum.topicBooks.map((b) => [b.title, b]));
  const spreads = new Set();
  for (const { bundle } of rows(run(40, 9))) {
    const sem = bundle.period.from < '2027-08' ? 1 : 2;
    const topics = new Set();
    for (const e of bundle.evidence) {
      if (e.kind === 'habit') continue;
      const b = books.get(e.topic);
      assert.ok(b && b.semester === sem, `학기 불일치 ${e.topic}`);
      topics.add(e.topic);
    }
    spreads.add(topics.size);
  }
  assert.ok(spreads.size > 1, '묶음마다 주제책 수가 다양해야 함');
});
