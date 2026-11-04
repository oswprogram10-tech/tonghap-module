// 평어 데이터 검증: JSON 스키마 + id 중복 + 템플릿 슬롯/종결 + 교과별 최소 개수 + 금지 표현과 템플릿의 충돌.
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const load = (p) => JSON.parse(readFileSync(resolve(root, p), 'utf8'));
const templates = load('data/evaluation/templates.json');
const forbidden = load('data/evaluation/forbidden.json');
const prompt = load('data/evaluation/prompt.json');
const curriculum = load('data/curriculum/integrated-2022.json');

const errors = [];
const ajv = new Ajv2020({ allErrors: true });
for (const [name, data] of [['templates', templates], ['forbidden', forbidden], ['prompt', prompt]]) {
  const schema = load(`data/evaluation/schema/${name}.schema.json`);
  if (!ajv.validate(schema, data)) for (const e of ajv.errors) errors.push(`${name} 스키마: ${e.instancePath || '/'} ${e.message}`);
}

if (errors.length === 0) {
  // 슬롯: 값이 항상 있는 슬롯(없어도 되는 것만 필요 조건으로 센다).
  const ALLOWED = { observation: ['topic', 'note', 'quote'], habit: ['title'], session: ['title', 'topic'] };
  const ALWAYS = { observation: [], habit: ['title'], session: ['title'] };
  const GROUPS = { habit: ['high', 'mid', 'low'], session: ['default'] };
  const slotsOf = (text) => [...text.matchAll(/\{([a-zA-Z]+)(?:\|([^}]+))?\}/g)].map((m) => m[1]).filter((s) => s !== 'E');
  const seen = new Set();
  const byGroup = new Map();
  const sample = { topic: '학교', note: '친구를 도와주는', quote: '왜 그래요?', title: '교실 정리' };
  for (const t of templates.templates) {
    if (seen.has(t.id)) errors.push(`템플릿 id 중복: ${t.id}`);
    seen.add(t.id);
    if (!t.text.endsWith('{E}') || t.text.split('{E}').length !== 2) errors.push(`${t.id}: {E}는 문장 끝에 한 번만 있어야 함`);
    const stray = t.text.replace(/\{[a-zA-Z]+(?:\|[^}]+)?\}/g, '').match(/[{}]/);
    if (stray) errors.push(`${t.id}: 짝이 맞지 않는 중괄호`);
    for (const s of slotsOf(t.text)) if (!ALLOWED[t.kind].includes(s)) errors.push(`${t.id}: ${t.kind}에서 쓸 수 없는 슬롯 {${s}}`);
    if (t.kind === 'observation') {
      const ok = t.group === 'general' || Object.values(templates.tagGroups[t.subject]).includes(t.group);
      if (!ok) errors.push(`${t.id}: 알 수 없는 묶음 ${t.group}`);
    } else if (!GROUPS[t.kind].includes(t.group)) errors.push(`${t.id}: ${t.kind}의 묶음은 ${GROUPS[t.kind].join('|')}이어야 함`);
    if (t.kind === 'habit' && t.subject !== '바') errors.push(`${t.id}: 습관 템플릿은 바른 생활만`);
    const key = `${t.kind}/${t.subject}/${t.group}`;
    const used = slotsOf(t.text);
    const needed = used.filter((s) => !ALWAYS[t.kind].includes(s));
    // 모든 문장이 근거 내용을 담도록: 관찰 템플릿은 {note}나 {quote}를 반드시 쓴다({topic}만으로는 안 됨).
    if (t.kind === 'observation' && !used.includes('note') && !used.includes('quote')) errors.push(`${t.id}: 관찰 템플릿은 {note} 또는 {quote}를 써야 함`);
    // "~하는 모습을 보임" 문체와 겹치지 않도록 틀 본문에 '모습'을 쓰지 않는다.
    if (t.text.includes('모습')) errors.push(`${t.id}: 틀 본문에 '모습'을 쓰지 않음(문체 ~하는 모습을 보임과 겹침)`);
    const e = byGroup.get(key) ?? { n: 0, free: 0, note: 0, quote: 0 };
    e.n += 1;
    if (needed.length === 0) e.free += 1;
    if (used.includes('note')) e.note += 1;
    if (used.includes('quote')) e.quote += 1;
    byGroup.set(key, e);
  }
  // 모든 (종류, 교과, 묶음): 전체 4개 이상(문장이 다양해지도록). 관찰은 {note} 2개·{quote} 1개 이상, 습관·차시는 슬롯 없이 쓸 수 있는 틀 2개 이상.
  const expected = [];
  for (const subj of ['바', '슬', '즐']) {
    for (const g of new Set([...Object.values(templates.tagGroups[subj]), 'general'])) expected.push(`observation/${subj}/${g}`);
    expected.push(`session/${subj}/default`);
  }
  for (const g of GROUPS.habit) expected.push(`habit/바/${g}`);
  for (const key of expected) {
    const e = byGroup.get(key);
    if (!e) errors.push(`템플릿 묶음 없음: ${key}`);
    else {
      const minTotal = key.startsWith('observation') ? 4 : 3;
      if (e.n < minTotal) errors.push(`${key}: 템플릿이 ${minTotal}개 이상 필요 (현재 ${e.n})`);
      if (key.startsWith('observation')) {
        if (e.note < 2) errors.push(`${key}: {note} 템플릿이 2개 이상 필요 (현재 ${e.note})`);
        if (e.quote < 1) errors.push(`${key}: {quote} 템플릿이 1개 이상 필요 (자막만 있는 기록용)`);
      } else if (e.free < 2) errors.push(`${key}: 슬롯 없이 쓸 수 있는 템플릿이 2개 이상 필요 (현재 ${e.free})`);
    }
  }
  for (const key of byGroup.keys()) if (!expected.includes(key)) errors.push(`정의되지 않은 묶음: ${key}`);

  // 금지 표현
  const seenF = new Set();
  const compiled = [];
  for (const f of forbidden.entries) {
    if (seenF.has(f.id)) errors.push(`금지 표현 id 중복: ${f.id}`);
    seenF.add(f.id);
    if (f.kind === 'regex') {
      if (/\(\?[=!<]|\\[1-9]/.test(f.pattern)) errors.push(`${f.id}: 앞뒤 확인·역참조는 쓸 수 없음`);
      try {
        compiled.push([f, new RegExp(f.pattern, 'u')]);
      } catch (err) {
        errors.push(`${f.id}: 정규식 오류 ${err.message}`);
      }
    } else compiled.push([f, null]);
  }
  // 템플릿에 금지 표현이 들어 있으면 안 된다(초안이 스스로 경고에 걸리면 안 됨).
  const ending = Object.values(templates.endings);
  for (const t of templates.templates) {
    for (const end of ending) {
      const text = t.text
        .replace(/\{E\}/, end)
        .replace(/\{([a-zA-Z]+)(?:\|[^}]+)?\}/g, (_, s) => sample[s] ?? '');
      for (const [f, re] of compiled) {
        const stripped = (f.except ?? []).reduce((acc, x) => acc.split(x).join(' '), text);
        if (re ? re.test(stripped) : stripped.includes(f.pattern)) errors.push(`${t.id}: 금지 표현 ${f.id}(${f.pattern}) 포함`);
      }
    }
  }
}

// 언어 모델 프롬프트: 자리표시자, 예시의 근거 id·성취기준 원문·문체 종결·금지 표현.
if (errors.length === 0) {
  for (const k of ['{target}', '{min}', '{max}']) if (!prompt.system.length.includes(k)) errors.push(`prompt.system.length에 ${k} 필요`);
  if (prompt.user.split('{bundle}').length !== 2) errors.push('prompt.user에 {bundle}이 한 번 있어야 함');
  const statements = new Map(curriculum.standards.map((s) => [s.code, s.statement]));
  const topics = new Set(curriculum.topicBooks.map((t) => t.title));
  const ids = new Set();
  const compiledF = forbidden.entries.map((f) => [f, f.kind === 'regex' ? new RegExp(f.pattern, 'u') : null]);
  const subjectsSeen = new Set();
  for (const ex of prompt.examples) {
    const where = `예시 ${ex.id}`;
    if (ids.has(ex.id)) errors.push(`${where}: id 중복`);
    ids.add(ex.id);
    subjectsSeen.add(ex.bundle.subject);
    const evIds = new Set();
    for (const e of ex.bundle.evidence) {
      if (evIds.has(e.id)) errors.push(`${where}: 근거 id 중복 ${e.id}`);
      evIds.add(e.id);
      if (e.topic && !topics.has(e.topic)) errors.push(`${where}: 알 수 없는 주제책 ${e.topic}`);
      for (const c of e.standards ?? []) {
        if (!ex.bundle.standardsText[c]) errors.push(`${where}: ${c}의 원문이 standardsText에 없음`);
        if (!c.includes(ex.bundle.subject)) errors.push(`${where}: ${c}는 ${ex.bundle.subject} 교과의 성취기준이 아님`);
      }
    }
    for (const [c, text] of Object.entries(ex.bundle.standardsText)) {
      if (statements.get(c) !== text) errors.push(`${where}: ${c} 원문이 교육과정 데이터와 다름`);
    }
    for (const [style, sentences] of Object.entries(ex.outputs)) {
      const end = prompt.system.styles[style].match(/'~([^']+?)\.'/)?.[1];
      for (const [i, s] of sentences.entries()) {
        const at = `${where} ${style} ${i + 1}번째 문장`;
        if (end && !s.text.endsWith(`${end}.`)) errors.push(`${at}: '~${end}.'로 끝나야 함`);
        if (/<think>|학생/.test(s.text)) errors.push(`${at}: '학생'·<think>는 쓸 수 없음`);
        for (const id of s.evidence) if (!evIds.has(id)) errors.push(`${at}: 예시 묶음에 없는 근거 id ${id}`);
        for (const [f, re] of compiledF) {
          const stripped = (f.except ?? []).reduce((acc, x) => acc.split(x).join(' '), s.text);
          if (re ? re.test(stripped) : stripped.includes(f.pattern)) errors.push(`${at}: 금지 표현 ${f.id}(${f.pattern})`);
        }
      }
      const total = sentences.reduce((n, s) => n + s.text.length, 0) + sentences.length - 1;
      if (total < 150 || total > 400) errors.push(`${where} ${style}: 전체 ${total}자(150~400자여야 함)`);
    }
  }
  if (subjectsSeen.size < 2) errors.push('예시는 교과 2개 이상을 다뤄야 함');
}

if (errors.length > 0) {
  console.error(`평어 데이터 검증 실패 (${errors.length}건)`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
console.log(`평어 데이터 검증 통과: 템플릿 ${templates.templates.length}개, 금지 표현 ${forbidden.entries.length}개, 프롬프트 예시 ${prompt.examples.length}개`);
