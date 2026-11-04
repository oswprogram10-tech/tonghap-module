// 합성 문장 은행(data/synthetic/phrases.json) 검증: 스키마 + 묶음 안 중복 + 이름 + 금지 표현 + 주제책별 차시 제목.
// 사용: node scripts/validate-synthetic.mjs [문장 은행 경로]  (기본 data/synthetic/phrases.json)
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const load = (p) => JSON.parse(readFileSync(resolve(root, p), 'utf8'));
const target = process.argv[2] ?? 'data/synthetic/phrases.json';
const phrases = JSON.parse(readFileSync(resolve(process.argv[2] ? process.cwd() : root, target), 'utf8'));
const schema = load('data/synthetic/schema/phrases.schema.json');
const forbidden = load('data/evaluation/forbidden.json');
const curriculum = load('data/curriculum/integrated-2022.json');

const errors = [];
const ajv = new Ajv2020({ allErrors: true });
if (!ajv.validate(schema, phrases)) for (const e of ajv.errors) errors.push(`스키마: ${e.instancePath || '/'} ${e.message}`);

if (errors.length === 0) {
  // 금지 표현
  const rules = forbidden.entries.map((e) => ({
    id: e.id,
    re: e.kind === 'regex' ? new RegExp(e.pattern) : null,
    word: e.kind === 'word' ? e.pattern : null,
    except: e.except ?? [],
  }));
  const hitForbidden = (text) => {
    for (const r of rules) {
      if (r.re) { if (r.re.test(text)) return r.id; continue; }
      let t = text;
      for (const ex of r.except) t = t.split(ex).join('');
      if (t.includes(r.word)) return r.id;
    }
    return null;
  };
  // 이름: 흔한 이름 목록 + 성 + 이름. 낱말 경계(뒤에 한글이 이어지지 않거나 조사)에서만 잡아 '지우개' 같은 낱말은 통과.
  const given = ['민준','서준','도윤','예준','시우','하준','주원','지호','지후','준서','건우','현우','우진','선우','유준','이준','서연','서윤','지우','하윤','지민','수아','지안','하은','아윤','채원','유진','민서','지유','수빈','예은','소율','다은','은서','서아','지아','예린','하린','나은','가은'];
  const surnames = '김이박최정강조윤장임한오서신권황안송류홍전고문양손배백허유남심노';
  const nameRe = new RegExp(`(?<![가-힣])(?:[${surnames}])?(?:${given.join('|')})(?:이|아|야)?(?:가|는|은|를|을|의|와|과|랑|도|에게|한테)?(?![가-힣])`);
  const anyText = [];
  const collect = (label, list) => list.forEach((s) => anyText.push([label, s]));
  for (const [g, list] of Object.entries(phrases.observations)) collect(`observations.${g}`, list);
  for (const [g, list] of Object.entries(phrases.quotes)) collect(`quotes.${g}`, list);
  for (const [g, list] of Object.entries(phrases.sessionTitles)) collect(`sessionTitles.${g}`, list);
  collect('habits', phrases.habits);
  for (const [label, s] of anyText) {
    const f = hitForbidden(s);
    if (f) errors.push(`${label}: 금지 표현(${f}) "${s}"`);
    const m = s.match(nameRe);
    if (m) errors.push(`${label}: 이름으로 보이는 낱말 "${m[0]}" in "${s}"`);
  }
  // 묶음 안 중복(공백 무시)
  const norm = (s) => s.replace(/\s+/g, '');
  const dupCheck = (label, list) => {
    const seen = new Set();
    for (const s of list) {
      const k = norm(s);
      if (seen.has(k)) errors.push(`${label}: 중복 "${s}"`);
      seen.add(k);
    }
  };
  for (const [g, list] of Object.entries(phrases.observations)) dupCheck(`observations.${g}`, list);
  for (const [g, list] of Object.entries(phrases.quotes)) dupCheck(`quotes.${g}`, list);
  for (const [g, list] of Object.entries(phrases.sessionTitles)) dupCheck(`sessionTitles.${g}`, list);
  dupCheck('habits', phrases.habits);
  // 주제책마다 차시 제목, 없는 주제책 id 금지
  const ids = new Set(curriculum.topicBooks.map((b) => b.id));
  for (const id of ids) if (!phrases.sessionTitles[id]) errors.push(`sessionTitles: 주제책 ${id}의 차시 제목이 없음`);
  for (const id of Object.keys(phrases.sessionTitles)) if (!ids.has(id)) errors.push(`sessionTitles: 교육과정에 없는 주제책 ${id}`);
}

if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log(`합성 문장 은행 검증 통과 (${target})`);
