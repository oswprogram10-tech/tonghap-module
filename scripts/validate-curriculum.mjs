// 교육과정 데이터 검증: JSON 스키마 + 데이터 간 정합성.
// 사용법: node scripts/validate-curriculum.mjs [데이터 파일 경로]
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dataPath = resolve(root, process.argv[2] ?? 'data/curriculum/integrated-2022.json');
const schemaPath = resolve(root, 'data/curriculum/schema/curriculum.schema.json');

const data = JSON.parse(readFileSync(dataPath, 'utf8'));
const schema = JSON.parse(readFileSync(schemaPath, 'utf8'));

const errors = [];
const ajv = new Ajv2020({ allErrors: true });
if (!ajv.validate(schema, data)) {
  for (const e of ajv.errors) errors.push(`스키마: ${e.instancePath || '/'} ${e.message}`);
}

if (errors.length === 0) {
  const subjectCodes = new Set(data.subjects.map((s) => s.code));
  const areaNumbers = new Set(data.areas.map((a) => a.number));

  const dup = (label, values) => {
    const seen = new Set();
    for (const v of values) {
      if (seen.has(v)) errors.push(`${label} 중복: ${v}`);
      seen.add(v);
    }
  };
  dup('교과 코드', data.subjects.map((s) => s.code));
  dup('교과 id', data.subjects.map((s) => s.id));
  dup('영역 번호', data.areas.map((a) => a.number));
  dup('주제책 id', data.topicBooks.map((t) => t.id));
  dup('주제책 학년·학기·순서', data.topicBooks.map((t) => `${t.grade}-${t.semester}-${t.order}`));
  dup('주제책 영역·번호', data.topicBooks.map((t) => `${t.area}-${t.standardIndex}`));
  dup('성취기준 코드', data.standards.map((s) => s.code));
  dup('단원 개발 도구 id', data.unitDevelopmentTools.map((t) => t.id));

  const pad = (n) => String(n).padStart(2, '0');
  for (const s of data.standards) {
    if (!subjectCodes.has(s.subject)) errors.push(`${s.code}: 없는 교과 '${s.subject}'`);
    if (!areaNumbers.has(s.area)) errors.push(`${s.code}: 없는 영역 ${s.area}`);
    const expected = `[2${s.subject}${pad(s.area)}-${pad(s.index)}]`;
    if (s.code !== expected) errors.push(`${s.code}: 코드가 교과·영역·번호와 불일치 (기대값 ${expected})`);
    if (s.verification === 'verified' && !s.statement) errors.push(`${s.code}: verified인데 원문(statement)이 없음`);
    if (s.statement && (/[\r\n]/.test(s.statement) || /\s{2,}/.test(s.statement) || s.statement !== s.statement.trim()))
      errors.push(`${s.code}: statement에 줄바꿈·연속 공백·앞뒤 공백이 있음`);
    for (const k of ['explanations', 'considerations'])
      for (const t of s[k] ?? []) {
        if (/[\r\n]/.test(t) || /\s{2,}/.test(t)) errors.push(`${s.code}: ${k}에 줄바꿈·연속 공백이 있음`);
        if (!t.trim()) errors.push(`${s.code}: ${k}에 빈 항목`);
      }
  }

  // 모든 주제책은 교과마다 같은 (영역, 번호)의 성취기준을 하나씩 가져야 한다.
  const byKey = new Map();
  for (const s of data.standards) {
    const key = `${s.area}-${s.index}`;
    if (!byKey.has(key)) byKey.set(key, new Set());
    byKey.get(key).add(s.subject);
  }
  for (const t of data.topicBooks) {
    if (!areaNumbers.has(t.area)) errors.push(`주제책 ${t.id}: 없는 영역 ${t.area}`);
    const subjects = byKey.get(`${t.area}-${t.standardIndex}`) ?? new Set();
    for (const code of subjectCodes) {
      if (!subjects.has(code)) errors.push(`주제책 ${t.id}: ${code} 교과의 ${t.area}영역 ${t.standardIndex}번 성취기준 없음`);
    }
  }
  // 반대로 모든 성취기준 묶음은 주제책 하나에 대응해야 한다.
  const topicKeys = new Set(data.topicBooks.map((t) => `${t.area}-${t.standardIndex}`));
  for (const key of byKey.keys()) {
    if (!topicKeys.has(key)) errors.push(`영역·번호 ${key}에 대응하는 주제책 없음`);
  }
}

if (errors.length > 0) {
  console.error(`검증 실패 (${errors.length}건): ${dataPath}`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}

const pending = data.standards.filter((s) => s.verification !== 'verified').length;
console.log(
  `검증 통과: 교과 ${data.subjects.length}, 영역 ${data.areas.length}, 주제책 ${data.topicBooks.length}, ` +
    `성취기준 ${data.standards.length} (원문 미확인 ${pending}), 단원 개발 도구 ${data.unitDevelopmentTools.length}`,
);
