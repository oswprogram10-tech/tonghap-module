// 합성 근거 묶음 생성기 (설계 09). 실제 학생 기록 없이 가짜 학생의 근거 묶음(설계 08 형식)을 JSONL로 만든다.
// 사용: node ml/synth/generate-bundles.mjs --students 200 --seed 1 --phrases data/synthetic/phrases.json --out ml/out/bundles.jsonl
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const readJson = (p) => JSON.parse(readFileSync(p, 'utf8'));

export const SUBJECTS = ['바', '슬', '즐'];
const PERIODS = {
  1: { from: '2027-03-02', to: '2027-07-18' },
  2: { from: '2027-08-23', to: '2027-12-24' },
};

export function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function makeRng(seed, index) {
  const r = mulberry32((Math.imul(seed | 0, 2654435761) ^ Math.imul(index + 1, 40503)) >>> 0);
  const rng = {
    next: r,
    int: (lo, hi) => lo + Math.floor(r() * (hi - lo + 1)),
    chance: (p) => r() < p,
    pick: (arr) => arr[Math.floor(r() * arr.length)],
    hex: (n) => Array.from({ length: n }, () => Math.floor(r() * 16).toString(16)).join(''),
  };
  // 겹치지 않게 하나씩 꺼내 쓰는 문장 뽑기(다 쓰면 다시 섞음).
  rng.shuffle = (arr) => {
    const a = [...arr];
    for (let i = a.length - 1; i > 0; i--) {
      const j = Math.floor(r() * (i + 1));
      [a[i], a[j]] = [a[j], a[i]];
    }
    return a;
  };
  return rng;
}

function dayNumber(iso) {
  return Math.floor(Date.parse(`${iso}T00:00:00Z`) / 86400000);
}
function isoOf(day) {
  return new Date(day * 86400000).toISOString().slice(0, 10);
}
function isWeekend(day) {
  const w = new Date(day * 86400000).getUTCDay();
  return w === 0 || w === 6;
}

/** 주제책 순서대로 학기를 같은 길이로 나눈 구간 안의 평일을 하나 고른다. */
function dateInSlice(rng, period, slice, slices) {
  const a = dayNumber(period.from);
  const b = dayNumber(period.to);
  const len = (b - a + 1) / slices;
  const lo = a + Math.floor(len * slice);
  const hi = Math.min(b, a + Math.floor(len * (slice + 1)) - 1);
  for (let i = 0; i < 20; i++) {
    const d = rng.int(lo, hi);
    if (!isWeekend(d)) return isoOf(d);
  }
  for (let d = lo; d <= hi; d++) if (!isWeekend(d)) return isoOf(d);
  return isoOf(lo);
}

export function loadContext({ phrasesPath }) {
  const curriculum = readJson(resolve(root, 'data/curriculum/integrated-2022.json'));
  const templates = readJson(resolve(root, 'data/evaluation/templates.json'));
  const phrases = readJson(phrasesPath);
  return { curriculum, tagGroups: templates.tagGroups, phrases };
}

function standardFor(curriculum, subject, book) {
  return curriculum.standards.find((s) => s.subject === subject && s.area === book.area && s.index === book.standardIndex) ?? null;
}

function habitProfile(rng) {
  const schoolDays = rng.int(20, 60);
  const p = rng.next();
  const rate = p < 0.45 ? 0.8 + rng.next() * 0.2 : p < 0.8 ? 0.5 + rng.next() * 0.3 : 0.15 + rng.next() * 0.35;
  return { schoolDays, doneDays: Math.min(schoolDays, Math.max(0, Math.round(schoolDays * rate))) };
}

export function makeBundle(ctx, rng, { grade, semester, subject, usedIds }) {
  const { curriculum, tagGroups, phrases } = ctx;
  const period = PERIODS[semester];
  const books = curriculum.topicBooks.filter((b) => b.grade === grade && b.semester === semester).sort((a, b) => a.order - b.order);
  const tags = Object.keys(tagGroups[subject]);
  const newId = (prefix) => {
    for (;;) {
      const id = `${prefix}:${rng.hex(12)}`;
      if (!usedIds.has(id)) {
        usedIds.add(id);
        return id;
      }
    }
  };

  const total = rng.int(3, 12);
  let habitN = 0;
  if (subject === '바') habitN = total >= 4 ? rng.int(1, 2) : 1;
  const sessionN = Math.min(total - habitN - 1, rng.int(0, 3));
  const obsN = total - habitN - sessionN;

  const standardsText = {};
  const note = (code, statement) => {
    standardsText[code] = statement;
  };
  const evidence = [];
  const pools = new Map(); // 문장 은행 이름 → 섞인 남은 문장
  const take = (bank, group) => {
    const key = `${bank}/${group}`;
    if (!pools.get(key)?.length) pools.set(key, rng.shuffle(phrases[bank][group]));
    return pools.get(key).pop();
  };

  // 주제책은 2~4권만 쓰고(학기 앞쪽부터 고르게), 학기를 책 수만큼 나눈 구간에서 날짜를 정한다.
  const bookCount = rng.int(Math.min(2, books.length), books.length);
  const start = rng.int(0, books.length - bookCount);
  const used = books.slice(start, start + bookCount);
  const sliceOf = (book) => books.indexOf(book);

  for (let i = 0; i < obsN; i++) {
    const book = used[i % used.length];
    const primary = rng.pick(tags);
    const obsTags = [primary];
    if (tags.length > 1 && rng.chance(0.35)) {
      const other = rng.pick(tags.filter((t) => t !== primary));
      obsTags.push(other);
    }
    const group = tagGroups[subject][primary];
    const std = rng.chance(0.7) ? standardFor(curriculum, subject, book) : null;
    if (std) note(std.code, std.statement);
    const ev = {
      id: newId('o'),
      kind: 'observation',
      date: dateInSlice(rng, period, sliceOf(book), books.length),
      topic: book.title,
      standards: std ? [std.code] : [],
      tags: obsTags,
      text: take('observations', group),
    };
    if (rng.chance(0.4)) ev.quote = take('quotes', group);
    ev.together = rng.chance(0.25);
    evidence.push(ev);
  }
  for (let i = 0; i < sessionN; i++) {
    const book = used[i % used.length];
    const std = standardFor(curriculum, subject, book);
    if (std) note(std.code, std.statement);
    evidence.push({
      id: newId('s'),
      kind: 'session',
      date: dateInSlice(rng, period, sliceOf(book), books.length),
      topic: book.title,
      title: take('sessionTitles', book.id),
      standards: std ? [std.code] : [],
    });
  }
  const habitTitles = rng.shuffle(phrases.habits);
  const habits = [];
  for (let i = 0; i < habitN; i++) {
    habits.push({ id: newId('h'), kind: 'habit', title: habitTitles[i], ...habitProfile(rng) });
  }
  // 앱과 같은 순서: 습관 먼저, 그 다음 날짜순(같으면 id순).
  const rest = evidence.sort((a, b) => (a.date === b.date ? (a.id < b.id ? -1 : 1) : a.date < b.date ? -1 : 1));
  const sortedCodes = Object.keys(standardsText).sort();
  return {
    student: { alias: '학생' },
    subject,
    period: { ...period },
    evidence: [...habits, ...rest],
    standardsText: Object.fromEntries(sortedCodes.map((c) => [c, standardsText[c]])),
  };
}

export function generate({ students, seed, phrasesPath }) {
  const ctx = loadContext({ phrasesPath });
  const usedIds = new Set();
  const lines = [];
  for (let i = 0; i < students; i++) {
    const rng = makeRng(seed, i);
    const grade = rng.int(1, 2);
    const semester = rng.int(1, 2);
    const syntheticStudent = `synth-${String(i + 1).padStart(4, '0')}`;
    for (const subject of SUBJECTS) {
      const bundle = makeBundle(ctx, rng, { grade, semester, subject, usedIds });
      lines.push(JSON.stringify({ meta: { syntheticStudent, subject, seed }, bundle }));
    }
  }
  return lines.join('\n') + '\n';
}

function parseArgs(argv) {
  const o = { students: 200, seed: 1, phrases: 'data/synthetic/phrases.json', out: 'ml/out/bundles.jsonl' };
  for (let i = 0; i < argv.length; i += 2) {
    const k = argv[i]?.replace(/^--/, '');
    if (!(k in o) || argv[i + 1] === undefined) throw new Error(`알 수 없는 옵션: ${argv[i]}`);
    o[k] = k === 'students' || k === 'seed' ? Number(argv[i + 1]) : argv[i + 1];
  }
  if (!Number.isInteger(o.students) || o.students < 1) throw new Error('--students는 1 이상의 정수');
  if (!Number.isInteger(o.seed)) throw new Error('--seed는 정수');
  return o;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const o = parseArgs(process.argv.slice(2));
    const text = generate({ students: o.students, seed: o.seed, phrasesPath: resolve(o.phrases) });
    const out = resolve(o.out);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, text);
    console.log(`묶음 ${o.students * 3}개를 ${o.out}에 만들었습니다 (시드 ${o.seed}).`);
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
