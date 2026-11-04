// 자막 정확도 지표: 정규화와 편집 거리. 설계: docs/design/17-stt-accuracy-eval.md
// 외부 의존성 없음(Node 내장만).

const STRIP = /[\p{P}\p{S}]/gu;

/** 공통 정규화: NFC, 영어 소문자, 문장부호·기호 제거, 연속 공백을 하나로. */
export function normalizeText(s) {
  return String(s ?? '')
    .normalize('NFC')
    .toLowerCase()
    .replace(STRIP, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

/** CER용: 정규화 후 공백도 모두 제거한 글자 배열. */
export function charUnits(s) {
  return [...normalizeText(s).replace(/\s+/g, '')];
}

/** WER용: 정규화 후 공백 기준 낱말 배열. */
export function wordUnits(s) {
  const n = normalizeText(s);
  return n === '' ? [] : n.split(' ');
}

/** 두 배열의 레벤슈타인 편집 거리(바꿈·넣음·뺌 각 1). */
export function editDistance(a, b) {
  if (a.length === 0) return b.length;
  if (b.length === 0) return a.length;
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost);
    }
    prev = cur;
  }
  return prev[b.length];
}

function score(ref, hyp) {
  const errors = editDistance(ref, hyp);
  const length = ref.length;
  // 기준이 비어 있으면: 결과도 비었으면 0, 아니면 1(나눌 수 없어 100%로 본다).
  const rate = length === 0 ? (hyp.length === 0 ? 0 : 1) : errors / length;
  return { errors, length, rate };
}

/** 글자 오류율. 반환: { errors, length, rate } */
export function cer(reference, hypothesis) {
  return score(charUnits(reference), charUnits(hypothesis));
}

/** 낱말 오류율(참고용). */
export function wer(reference, hypothesis) {
  return score(wordUnits(reference), wordUnits(hypothesis));
}

/** 여러 항목의 합산 비율(오류 합 / 기준 길이 합). 기준 길이 합이 0이면 null. */
export function aggregate(scores) {
  const errors = scores.reduce((a, s) => a + s.errors, 0);
  const length = scores.reduce((a, s) => a + s.length, 0);
  return { errors, length, rate: length === 0 ? null : errors / length };
}
