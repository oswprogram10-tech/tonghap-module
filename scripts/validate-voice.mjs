// 안내 음성 검증: cues.json 스키마 + id 중복 + 모든 id의 기본 음성 파일 존재 + 크기 합계 보고.
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const data = JSON.parse(readFileSync(resolve(root, 'data/voice/cues.json'), 'utf8'));
const schema = JSON.parse(readFileSync(resolve(root, 'data/voice/schema/cues.schema.json'), 'utf8'));
const dir = resolve(root, 'src/board/voice');

const errors = [];
const ajv = new Ajv2020({ allErrors: true });
if (!ajv.validate(schema, data)) for (const e of ajv.errors) errors.push(`스키마: ${e.instancePath || '/'} ${e.message}`);

let total = 0;
if (errors.length === 0) {
  const seen = new Set();
  const files = existsSync(dir) ? readdirSync(dir) : [];
  for (const c of data.cues) {
    if (seen.has(c.id)) errors.push(`id 중복: ${c.id}`);
    seen.add(c.id);
    const name = ['ogg', 'wav'].map((ext) => `${c.id}.${ext}`).find((f) => files.includes(f));
    if (!name) {
      errors.push(`${c.id}: 기본 음성 파일(src/board/voice/${c.id}.ogg)이 없습니다`);
      continue;
    }
    const size = statSync(resolve(dir, name)).size;
    if (size < 1000) errors.push(`${name}: 파일이 너무 작습니다(${size}바이트)`);
    total += size;
  }
  for (const f of files) if (!seen.has(f.replace(/\.(ogg|wav)$/, ''))) errors.push(`${f}: cues.json에 없는 음성 파일`);
}

if (errors.length > 0) {
  console.error(`안내 음성 검증 실패 (${errors.length}건)`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
console.log(`안내 음성 검증 통과: 문구 ${data.cues.length}개, 기본 음성 합계 ${(total / 1024).toFixed(1)}KB`);
