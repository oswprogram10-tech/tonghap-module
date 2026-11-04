// 실행: npm test
import test from 'node:test';
import assert from 'node:assert/strict';
import { cer, wer, aggregate, editDistance, normalizeText } from './metrics.mjs';

test('같은 문장은 CER·WER 0', () => {
  assert.equal(cer('개미가 줄을 서서 가요', '개미가 줄을 서서 가요').rate, 0);
  assert.equal(wer('개미가 줄을 서서 가요', '개미가 줄을 서서 가요').rate, 0);
});

test('한 글자 바꿈은 1/기준 글자 수', () => {
  const r = cer('개미가줄', '개미가술');
  assert.equal(r.errors, 1);
  assert.equal(r.length, 4);
  assert.equal(r.rate, 0.25);
});

test('띄어쓰기와 문장부호만 다르면 CER 0, WER는 0 초과', () => {
  assert.equal(cer('개미가 줄을 서서 가요.', '개미가줄을 서서가요').rate, 0);
  assert.ok(wer('개미가 줄을 서서 가요', '개미가줄을 서서가요').rate > 0);
});

test('빈 기준: 둘 다 비면 0, 결과가 있으면 1', () => {
  assert.equal(cer('', '').rate, 0);
  assert.equal(cer('  .', '').rate, 0);
  assert.equal(cer('', '가').rate, 1);
  assert.equal(wer('', '').rate, 0);
});

test('빈 결과는 기준 전체가 오류(CER 1)', () => {
  assert.equal(cer('개미', '').rate, 1);
});

test('한글 조합형(NFD) 입력도 완성형과 같게 본다', () => {
  const nfd = '개미가 줄'.normalize('NFD');
  assert.notEqual(nfd, '개미가 줄');
  assert.equal(cer('개미가 줄', nfd).rate, 0);
  assert.equal(normalizeText(nfd), '개미가 줄');
});

test('영어는 소문자로 맞추고 숫자는 그대로 둔다', () => {
  assert.equal(cer('Hello 3', 'hello 3').rate, 0);
  assert.equal(cer('Hello 3', 'hello 4').errors, 1);
});

test('편집 거리 기본 값', () => {
  assert.equal(editDistance([...'kitten'], [...'sitting']), 3);
  assert.equal(editDistance([], ['a']), 1);
});

test('합산 비율은 오류 합 / 길이 합', () => {
  const a = aggregate([cer('가나다라', '가나다'), cer('마바', '마바')]);
  assert.equal(a.errors, 1);
  assert.equal(a.length, 6);
  assert.equal(aggregate([]).rate, null);
});
