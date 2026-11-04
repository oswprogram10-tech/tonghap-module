// 평어 문체 예시(ml/data/style-examples.jsonl) 검증.
// 생기부 기재요령: 명사형 어미로 종결, 특수문자·번호 기호 지양, 1·2학년 바·슬·즐은 통합 입력(교과 누락 금지).
// 사용: node scripts/validate-style-examples.mjs [파일 경로]
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const target = process.argv[2] ? resolve(process.cwd(), process.argv[2]) : resolve(root, 'ml/data/style-examples.jsonl');
const forbidden = JSON.parse(readFileSync(resolve(root, 'data/evaluation/forbidden.json'), 'utf8'));
const curriculum = JSON.parse(readFileSync(resolve(root, 'data/curriculum/integrated-2022.json'), 'utf8'));

const codes = new Set(curriculum.standards.map((s) => s.code));
const topics = new Map(curriculum.topicBooks.map((b) => [b.title, b]));
const rules = forbidden.entries.map((e) => ({ id: e.id, re: e.kind === 'regex' ? new RegExp(e.pattern) : null, word: e.kind === 'word' ? e.pattern : null, except: e.except ?? [] }));
const hitForbidden = (text) => {
  for (const r of rules) {
    if (r.re) { if (r.re.test(text)) return r.id; continue; }
    let t = text;
    for (const ex of r.except) t = t.split(ex).join('');
    if (t.includes(r.word)) return r.id;
  }
  return null;
};

/** 한글 음절의 받침 번호(0=없음, 16=ㅁ). */
const jong = (ch) => { const c = ch.charCodeAt(0) - 0xac00; return c >= 0 && c < 11172 ? c % 28 : -1; };
export const sentences = (text) => text.split(/(?<=\.)\s+/).map((s) => s.trim()).filter(Boolean);
/** 명사형 종결: 마침표 앞 글자가 ㅁ 받침(함, 음, 됨, 보임, 느낌, 씀…). */
export const nominalEnding = (s) => s.endsWith('.') && jong(s.at(-2)) === 16;

const LIMITS = { subject: [40, 200], integrated: [200, 500] };
const errors = [];
const seen = new Set();
const lines = readFileSync(target, 'utf8').split('\n').filter((l) => l.trim());
const count = { subject: 0, integrated: 0, ham: 0, eum: 0, boim: 0 };

lines.forEach((line, i) => {
  const at = `${i + 1}행`;
  let o;
  try { o = JSON.parse(line); } catch (e) { errors.push(`${at}: JSON 오류 ${e.message}`); return; }
  const id = o.id ?? at;
  const err = (m) => errors.push(`${id}: ${m}`);
  if (seen.has(o.id)) err('id 중복');
  seen.add(o.id);
  if (o.source !== 'ai-draft' && o.source !== 'teacher') err('source는 ai-draft 또는 teacher');
  if (!['subject', 'integrated'].includes(o.kind)) { err('kind는 subject 또는 integrated'); return; }
  if (!['ham', 'eum', 'boim'].includes(o.style)) err('style은 ham, eum, boim');
  if (![1, 2].includes(o.grade) || ![1, 2].includes(o.semester)) err('grade·semester는 1 또는 2');
  count[o.kind]++; count[o.style]++;

  for (const t of o.topics ?? []) {
    const b = topics.get(t);
    if (!b) { err(`없는 주제책 ${t}`); continue; }
    if (b.grade !== o.grade || b.semester !== o.semester) err(`주제책 ${t}: ${b.grade}학년 ${b.semester}학기 주제책임`);
  }
  for (const c of o.standards ?? []) if (!codes.has(c)) err(`없는 성취기준 ${c}`);
  if (o.kind === 'subject') {
    if (!['바', '슬', '즐'].includes(o.subject)) err('subject는 바, 슬, 즐');
    if (!(o.standards ?? []).every((c) => c.startsWith(`[2${o.subject}`))) err('성취기준 교과가 subject와 다름');
  } else {
    for (const s of ['바', '슬', '즐']) if (!(o.standards ?? []).some((c) => c.startsWith(`[2${s}`))) err(`통합본에 ${s} 교과 성취기준이 없음(교과 누락)`);
  }

  const text = o.text ?? '';
  const [min, max] = LIMITS[o.kind];
  const n = [...text].length;
  if (n < min || n > max) err(`글자 수 ${n}(허용 ${min}~${max})`);
  const f = hitForbidden(text);
  if (f) err(`금지 표현(${f})`);
  if (/학생|○○/.test(text)) err('「학생」이나 이름 자리표시를 쓰지 않음(주어 생략)');
  if (/[‘’“”"'•※*①-⑳\-\n]/.test(text) || /^\s*\d+[.)]/.test(text)) err('특수문자·번호 기호·줄바꿈을 쓰지 않음');
  const ss = sentences(text);
  ss.forEach((s, k) => { if (!nominalEnding(s)) err(`${k + 1}번째 문장이 명사형으로 끝나지 않음: …${s.slice(-8)}`); });
  if (o.style === 'eum' && !ss.every((s) => s.endsWith('음.'))) err('eum 문체는 모든 문장이 「~음.」');
  if (o.style === 'boim' && !text.endsWith('모습을 보임.')) err('boim 문체는 「~하는 모습을 보임.」으로 끝남');
  if (o.style === 'ham' && ss.some((s) => s.endsWith('였음.') || s.endsWith('었음.') || s.endsWith('았음.'))) err('ham 문체에 「~하였음」이 섞임');
});

if (errors.length) {
  for (const e of errors) console.error(`오류: ${e}`);
  process.exit(1);
}
console.log(`평어 문체 예시 검증 통과: ${lines.length}개 (교과별 ${count.subject}, 통합 ${count.integrated} / 함 ${count.ham}, 하였음 ${count.eum}, 모습을 보임 ${count.boim})`);
