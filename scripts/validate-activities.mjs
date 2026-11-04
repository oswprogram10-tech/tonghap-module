// 활동 카드 검증: 스키마, id 중복, 주제책·성취기준이 교육과정과 맞는지, 금지 표현.
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => JSON.parse(readFileSync(resolve(root, p), 'utf8'));
const data = read('data/activities/activities.json');
const schema = read('data/activities/schema/activities.schema.json');
const curriculum = read('data/curriculum/integrated-2022.json');
const forbidden = read('data/evaluation/forbidden.json');

const errors = [];
const ajv = new Ajv2020({ allErrors: true });
if (!ajv.validate(schema, data)) for (const e of ajv.errors) errors.push(`스키마: ${e.instancePath || '/'} ${e.message}`);

if (errors.length === 0) {
  const topics = new Map(curriculum.topicBooks.map((t) => [t.id, t]));
  const words = forbidden.entries.filter((e) => e.kind === 'word');
  const ids = new Set();
  const titles = new Set();
  for (const a of data.activities) {
    if (ids.has(a.id)) errors.push(`id 중복: ${a.id}`);
    ids.add(a.id);
    const key = `${a.topicBookId}/${a.title}`;
    if (titles.has(key)) errors.push(`같은 주제책에 같은 제목: ${key}`);
    titles.add(key);
    const t = topics.get(a.topicBookId);
    if (!t) {
      errors.push(`${a.id}: 없는 주제책 ${a.topicBookId}`);
      continue;
    }
    // 주제책의 성취기준: 교과마다 같은 (영역, 번호)
    const own = curriculum.subjects.map((s) => `[2${s.code}${String(t.area).padStart(2, '0')}-${String(t.standardIndex).padStart(2, '0')}]`);
    for (const c of a.standards) if (!own.includes(c)) errors.push(`${a.id}: ${c}는 주제책 「${t.title}」의 성취기준이 아님 (${own.join(', ')})`);
    if (!a.standards[0].startsWith(`[2${a.subject}`)) errors.push(`${a.id}: 첫 성취기준이 ${a.subject} 교과가 아님`);
    if (new Set(a.standards).size !== a.standards.length) errors.push(`${a.id}: 성취기준 중복`);
    const text = [a.title, ...a.steps].join(' ');
    for (const w of words) {
      if (text.includes(w.pattern) && !(w.except ?? []).some((x) => text.includes(x))) errors.push(`${a.id}: 금지 표현 '${w.pattern}'`);
    }
  }
}

if (errors.length > 0) {
  console.error(`활동 카드 검증 실패 (${errors.length}건)`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
const byKind = (k) => data.activities.filter((a) => a.kind === k).length;
console.log(`활동 카드 검증 통과: ${data.activities.length}개 (일반 ${byKind('general')}, 안전 ${byKind('safety')}, 메이킹북 ${byKind('makingbook')})`);
