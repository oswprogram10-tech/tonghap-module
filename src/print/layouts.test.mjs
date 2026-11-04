// 실행: npm test
import test from 'node:test';
import assert from 'node:assert/strict';
import { layoutFor, wrapText, GUIDE_MM, TEMPLATE_PAGES } from './layouts.ts';

const spec = (template) => ({ template, title: '우리 학교', pages: Array.from({ length: TEMPLATE_PAGES[template] }, (_, i) => `안내 ${i + 1}번을 그려요`) });
const overlap = (a, b) => a.x < b.x + b.w - 1e-9 && b.x < a.x + a.w - 1e-9 && a.y < b.y + b.h - 1e-9 && b.y < a.y + a.h - 1e-9;

test('칸 수: zine8 8칸, card2 2면, accordion4 4칸', () => {
  assert.equal(layoutFor(spec('zine8')).cells.length, 8);
  assert.equal(layoutFor(spec('card2')).cells.length, 2);
  assert.equal(layoutFor(spec('accordion4')).cells.length, 4);
});

test('종이 방향: 세 틀 모두 가로 A4', () => {
  for (const t of ['zine8', 'card2', 'accordion4']) assert.deepEqual(layoutFor(spec(t)).paper, { w: 297, h: 210, orientation: 'landscape' });
});

test('zine8 한 쪽은 74.25 x 105mm', () => {
  for (const c of layoutFor(spec('zine8')).cells) assert.deepEqual([c.w, c.h], [74.25, 105]);
});

test('zine8 윗줄 4칸은 180도 돌고 아랫줄은 그대로다', () => {
  const { cells, paper } = layoutFor(spec('zine8'));
  const top = cells.filter((c) => c.y < paper.h / 2);
  const bottom = cells.filter((c) => c.y >= paper.h / 2);
  assert.equal(top.length, 4);
  assert.ok(top.every((c) => c.rotate === 180));
  assert.ok(bottom.every((c) => c.rotate === 0));
});

test('zine8 쪽 순서: 표지(1)·뒷표지(8)와 속 6쪽에 안내가 순서대로 들어간다', () => {
  const { cells } = layoutFor(spec('zine8'));
  assert.deepEqual(cells.map((c) => c.index), [1, 2, 3, 4, 5, 6, 7, 8]);
  assert.equal(cells[0].kind, 'cover');
  assert.ok(cells[0].title && cells[0].name);
  assert.equal(cells[7].kind, 'back');
  assert.equal(cells[7].text, '다 만들었어요!');
  assert.ok(cells[7].date);
  assert.deepEqual(cells.slice(1, 7).map((c) => c.text), spec('zine8').pages);
  // 접이 책 표준 배치: 아랫줄 6 7 8 1, 윗줄(거꾸로) 5 4 3 2
  const row = (r) => cells.filter((c) => (c.y > 0) === r).sort((a, b) => a.x - b.x).map((c) => c.index);
  assert.deepEqual(row(true), [6, 7, 8, 1]);
  assert.deepEqual(row(false), [5, 4, 3, 2]);
});

test('zine8 접는 선 6개와 자르는 선 1개(가운데 두 칸 사이)', () => {
  const { lines, paper } = layoutFor(spec('zine8'));
  const cuts = lines.filter((l) => l.kind === 'cut');
  assert.equal(cuts.length, 1);
  assert.deepEqual([cuts[0].x1, cuts[0].x2, cuts[0].y1, cuts[0].y2], [74.25, 222.75, paper.h / 2, paper.h / 2]);
  assert.equal(lines.filter((l) => l.kind === 'fold').length, 5);
});

test('칸은 A4 안에 들어가고 서로 겹치지 않는다', () => {
  for (const t of ['zine8', 'card2', 'accordion4']) {
    const { cells, paper } = layoutFor(spec(t));
    for (const c of cells) {
      assert.ok(c.x >= 0 && c.y >= 0 && c.x + c.w <= paper.w + 1e-9 && c.y + c.h <= paper.h + 1e-9, `${t} ${c.index} 밖으로 나감`);
    }
    for (let i = 0; i < cells.length; i++) for (let j = i + 1; j < cells.length; j++) assert.ok(!overlap(cells[i], cells[j]), `${t} ${i}-${j} 겹침`);
    const area = cells.reduce((s, c) => s + c.w * c.h, 0);
    assert.ok(Math.abs(area - paper.w * paper.h) < 1e-6, `${t} 칸이 종이를 가득 채움`);
  }
});

test('card2는 앞면 그림 칸과 안쪽 쓰기 줄, accordion4는 첫 칸에 제목·이름', () => {
  const card = layoutFor(spec('card2')).cells;
  assert.ok(card[0].title && card[0].picture && !card[0].lines);
  assert.ok(card[1].lines && !card[1].picture);
  const acc = layoutFor(spec('accordion4')).cells;
  assert.ok(acc[0].title && acc[0].name);
  assert.ok(acc.slice(1).every((c) => !c.title && !c.name));
  assert.ok(acc.every((c) => c.picture && c.text));
});

test('줄 나누기는 폭을 넘지 않고 글자를 잃지 않는다', () => {
  const text = '학교 정문을 그려요 그리고 이야기를 써요';
  const lines = wrapText(text, 44, GUIDE_MM);
  assert.ok(lines.length > 1);
  assert.ok(lines.every((l) => [...l].length <= Math.floor(44 / GUIDE_MM)));
  assert.equal(lines.join('').replace(/\s/g, ''), text.replace(/\s/g, ''));
  assert.deepEqual(wrapText('', 44, GUIDE_MM), ['']);
});
