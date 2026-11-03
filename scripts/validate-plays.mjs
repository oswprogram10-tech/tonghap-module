// 놀이 카드 데이터 검증: JSON 스키마 + id 중복 + 주제책 id가 교육과정 데이터에 있는지.
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const data = JSON.parse(readFileSync(resolve(root, 'data/plays/plays.json'), 'utf8'));
const schema = JSON.parse(readFileSync(resolve(root, 'data/plays/schema/plays.schema.json'), 'utf8'));
const curriculum = JSON.parse(readFileSync(resolve(root, 'data/curriculum/integrated-2022.json'), 'utf8'));

const errors = [];
const ajv = new Ajv2020({ allErrors: true });
if (!ajv.validate(schema, data)) for (const e of ajv.errors) errors.push(`스키마: ${e.instancePath || '/'} ${e.message}`);

if (errors.length === 0) {
  const topicIds = new Set(curriculum.topicBooks.map((t) => t.id));
  const seenId = new Set();
  const seenName = new Set();
  for (const p of data.plays) {
    if (seenId.has(p.id)) errors.push(`id 중복: ${p.id}`);
    if (seenName.has(p.name)) errors.push(`이름 중복: ${p.name}`);
    seenId.add(p.id);
    seenName.add(p.name);
    for (const t of p.topicBookIds) if (!topicIds.has(t)) errors.push(`${p.id}: 없는 주제책 ${t}`);
    if (new Set(p.tags).size !== p.tags.length) errors.push(`${p.id}: 태그 중복`);
  }
}

if (errors.length > 0) {
  console.error(`놀이 카드 검증 실패 (${errors.length}건)`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
const count = (k, v) => data.plays.filter((p) => p[k] === v).length;
console.log(
  `놀이 카드 검증 통과: ${data.plays.length}개 (실외 ${count('place', 'outdoor')}, 실내 ${count('place', 'indoor')}, 둘 다 ${count('place', 'both')})`,
);
