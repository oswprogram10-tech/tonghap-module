// 자막 정확도 측정 (개발자용). 설계: docs/design/17-stt-accuracy-eval.md
// 실행: node ml/stt/eval.mjs --set <폴더> --whisper <whisper-cli> --model <ggml> [--model <ggml>] [--threads 4] [--out <폴더>]
//       node ml/stt/eval.mjs --set <폴더> --dry-run      (manifest 읽기·검사만, whisper 불필요)
// 인자 틀은 whisper-args.json(Rust의 WHISPER_ARGS와 테스트로 같게 유지). 인자에 한국어를 넣지 않는다(윈도우 CP949).
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import Ajv from 'ajv';
import { cer, wer, aggregate } from './metrics.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const NAME_LIKE = /^[가-힣]{2,4}$/;

function parseArgs(argv) {
  const o = { models: [], threads: 4, dryRun: false };
  const need = (i) => {
    if (i + 1 >= argv.length) throw new Error(`${argv[i]} 값이 없습니다`);
    return argv[i + 1];
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--set') o.set = need(i++);
    else if (a === '--whisper') o.whisper = need(i++);
    else if (a === '--model') o.models.push(need(i++));
    else if (a === '--threads') o.threads = Number(need(i++));
    else if (a === '--out') o.out = need(i++);
    else if (a === '--max-cer') o.maxCer = Number(need(i++));
    else if (a === '--dry-run') o.dryRun = true;
    else throw new Error(`알 수 없는 옵션: ${a}`);
  }
  if (!o.set) throw new Error('--set <평가 묶음 폴더>가 필요합니다');
  if (!Number.isInteger(o.threads) || o.threads < 1) throw new Error('--threads는 1 이상의 정수');
  if (!o.dryRun && (!o.whisper || o.models.length === 0)) throw new Error('--whisper와 --model이 필요합니다(검사만 하려면 --dry-run)');
  return o;
}

export function loadManifest(dir) {
  const file = path.join(dir, 'manifest.json');
  const manifest = JSON.parse(fs.readFileSync(file, 'utf8'));
  const schema = JSON.parse(fs.readFileSync(path.join(here, 'manifest.schema.json'), 'utf8'));
  const validate = new Ajv({ allErrors: true }).compile(schema);
  if (!validate(manifest)) {
    const msg = validate.errors.map((e) => `${e.instancePath || '/'} ${e.message}`).join('; ');
    throw new Error(`manifest.json이 형식에 맞지 않습니다: ${msg}`);
  }
  const ids = new Set();
  const warnings = [];
  for (const it of manifest.items) {
    if (ids.has(it.id)) throw new Error(`id가 겹칩니다: ${it.id}`);
    ids.add(it.id);
    if (it.speaker && NAME_LIKE.test(it.speaker)) warnings.push(`항목 ${it.id}: speaker "${it.speaker}"가 이름처럼 보입니다. 익명 기호(child-1)를 쓰세요`);
  }
  return { manifest, warnings };
}

/** 16kHz 모노 16비트 WAV인지 확인하고 길이(초)를 돌려준다. 아니면 오류. */
export function wavSeconds(file) {
  const b = fs.readFileSync(file);
  if (b.length < 44 || b.toString('latin1', 0, 4) !== 'RIFF' || b.toString('latin1', 8, 12) !== 'WAVE') throw new Error('WAV 파일이 아닙니다');
  let pos = 12;
  let fmt = null;
  while (pos + 8 <= b.length) {
    const id = b.toString('latin1', pos, pos + 4);
    const size = b.readUInt32LE(pos + 4);
    if (id === 'fmt ') fmt = { tag: b.readUInt16LE(pos + 8), ch: b.readUInt16LE(pos + 10), rate: b.readUInt32LE(pos + 12), bits: b.readUInt16LE(pos + 22) };
    if (id === 'data') {
      if (!fmt) throw new Error('fmt 청크가 없습니다');
      if (fmt.tag !== 1 || fmt.ch !== 1 || fmt.rate !== 16000 || fmt.bits !== 16) {
        throw new Error(`16kHz 모노 16비트가 아닙니다(${fmt.rate}Hz, ${fmt.ch}채널, ${fmt.bits}비트)`);
      }
      const dataSize = Math.min(size, b.length - pos - 8);
      return dataSize / 32000;
    }
    pos += 8 + size + (size % 2);
  }
  throw new Error('data 청크가 없습니다');
}

export function buildArgs(template, { model, wav, lang, threads }) {
  const map = { '{model}': model, '{wav}': wav, '{lang}': lang, '{threads}': String(threads) };
  return template.map((a) => (a in map ? map[a] : a));
}

/** 앱의 clean_transcript와 같은 취지: [BLANK_AUDIO]·(웃음) 같은 표시만 있는 줄을 빼고 한 줄로 합친다. */
export function cleanTranscript(raw) {
  return raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !/^[[(][^\])]*[\])]$/.test(l))
    .join(' ');
}

function runOne(o, template, model, item, wavFile) {
  const args = buildArgs(template, { model, wav: wavFile, lang: item.lang ?? 'ko', threads: o.threads });
  const t0 = process.hrtime.bigint();
  const r = spawnSync(o.whisper, args, { encoding: 'utf8', timeout: 10 * 60 * 1000, maxBuffer: 64 * 1024 * 1024 });
  const seconds = Number(process.hrtime.bigint() - t0) / 1e9;
  if (r.error) throw new Error(`whisper-cli를 실행하지 못했습니다: ${r.error.message}`);
  if (r.status !== 0) throw new Error(`whisper-cli 종료 코드 ${r.status}: ${(r.stderr || '').slice(-500)}`);
  return { hyp: cleanTranscript(r.stdout), seconds };
}

const pct = (x) => (x == null ? '-' : `${(x * 100).toFixed(1)}%`);

function makeReport(manifest, results, skipped) {
  const L = [`# 자막 정확도 보고서: ${manifest.name}`, '', `- 동의 기록: ${manifest.consent ?? '(없음)'}`, `- 측정 항목 ${results[0]?.items.length ?? 0}개, 건너뜀 ${skipped.length}개`, ''];
  if (skipped.length) L.push(`건너뜀(음성 파일 없음): ${skipped.join(', ')}`, '');
  for (const m of results) {
    L.push(`## 모델: ${m.model}`, '', `- CER ${pct(m.cer.rate)}, WER ${pct(m.wer.rate)}, 평균 배율 ${m.avgRatio == null ? '-' : m.avgRatio.toFixed(2)}x(실행 시간 / 음성 길이)`, '');
    const tags = Object.entries(m.byTag);
    if (tags.length) {
      L.push('| 태그 | 항목 수 | CER |', '|---|---|---|');
      for (const [t, v] of tags) L.push(`| ${t} | ${v.count} | ${pct(v.cer)} |`);
      L.push('');
    }
    L.push('### 가장 틀린 항목', '');
    for (const it of m.worst) L.push(`- ${it.id} (CER ${pct(it.cer)})`, `  - 기준: ${it.text}`, `  - 결과: ${it.hyp}`);
    L.push('');
  }
  return L.join('\n');
}

export function summarize(model, items) {
  const byTag = {};
  const tagScores = {};
  for (const it of items) for (const t of it.tags ?? []) (tagScores[t] ??= []).push(it.cerScore);
  for (const [t, s] of Object.entries(tagScores)) byTag[t] = { count: s.length, cer: aggregate(s).rate };
  const ratios = items.filter((i) => i.audioSeconds > 0).map((i) => i.seconds / i.audioSeconds);
  return {
    model,
    cer: aggregate(items.map((i) => i.cerScore)),
    wer: aggregate(items.map((i) => i.werScore)),
    avgRatio: ratios.length ? ratios.reduce((a, b) => a + b, 0) / ratios.length : null,
    byTag,
    worst: [...items].sort((a, b) => b.cer - a.cer).slice(0, 10),
    items,
  };
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  const dir = path.resolve(o.set);
  const { manifest, warnings } = loadManifest(dir);
  for (const w of warnings) console.warn(`경고: ${w}`);

  const present = [];
  const skipped = [];
  for (const it of manifest.items) {
    const f = path.resolve(dir, it.wav);
    if (!fs.existsSync(f)) {
      skipped.push(it.id);
      continue;
    }
    try {
      present.push({ item: it, file: f, audioSeconds: wavSeconds(f) });
    } catch (e) {
      throw new Error(`항목 ${it.id}의 음성 ${it.wav}: ${e.message}`);
    }
  }
  console.log(`묶음 "${manifest.name}": 항목 ${manifest.items.length}개, 음성 있음 ${present.length}개, 건너뜀 ${skipped.length}개`);
  if (o.dryRun) return;
  if (present.length === 0) throw new Error('음성 파일이 있는 항목이 없습니다');

  const template = JSON.parse(fs.readFileSync(path.join(here, 'whisper-args.json'), 'utf8')).args;
  const results = [];
  for (const model of o.models) {
    const items = [];
    for (const p of present) {
      const { hyp, seconds } = runOne(o, template, path.resolve(model), p.item, p.file);
      const cerScore = cer(p.item.text, hyp);
      const werScore = wer(p.item.text, hyp);
      console.log(`${path.basename(model)} ${p.item.id}: CER ${pct(cerScore.rate)} (${seconds.toFixed(1)}초)`);
      items.push({ id: p.item.id, tags: p.item.tags ?? [], text: p.item.text, hyp, cer: cerScore.rate, cerScore, werScore, seconds, audioSeconds: p.audioSeconds });
    }
    results.push(summarize(path.basename(model), items));
  }

  const out = path.resolve(o.out ?? path.join(here, '..', 'out', 'stt'));
  fs.mkdirSync(out, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const base = path.join(out, `report-${stamp}`);
  fs.writeFileSync(`${base}.md`, makeReport(manifest, results, skipped));
  fs.writeFileSync(`${base}.json`, JSON.stringify({ name: manifest.name, skipped, results }, null, 2));
  console.log(`보고서: ${base}.md`);

  if (o.maxCer != null) {
    const bad = results.filter((m) => m.cer.rate == null || m.cer.rate > o.maxCer);
    if (bad.length) throw new Error(`CER가 상한 ${o.maxCer}를 넘었습니다: ${bad.map((m) => `${m.model} ${pct(m.cer.rate)}`).join(', ')}`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`오류: ${e.message}`);
    process.exit(1);
  });
}
