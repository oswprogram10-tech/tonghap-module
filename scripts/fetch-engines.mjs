// 점검 도구가 쓰는 llama.cpp·whisper.cpp 윈도우 실행 파일을 내려받아
// src-tauri/resources/engines/{llama,whisper}/ 에 넣는다. 버전은 src-tauri/engines.json에 고정.
// 실행 파일 하나(main)와 같은 폴더의 DLL만 남겨 설치 파일 크기를 줄인다.
// 사용법: node scripts/fetch-engines.mjs   (윈도우 전용. GITHUB_TOKEN이 있으면 API 요청에 사용)
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const config = JSON.parse(readFileSync(join(root, 'src-tauri/engines.json'), 'utf8'));
const outRoot = join(root, 'src-tauri/resources/engines');

if (process.platform !== 'win32') {
  console.log('윈도우가 아니어서 엔진 내려받기를 건너뜁니다(윈도우용 실행 파일만 배포).');
  process.exit(0);
}

const headers = { 'User-Agent': 'tonghap-module-build', Accept: 'application/vnd.github+json' };
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

function findFile(dir, name) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) {
      const found = findFile(p, name);
      if (found) return found;
    } else if (entry.toLowerCase() === name.toLowerCase()) {
      return p;
    }
  }
  return null;
}

const versions = {};
rmSync(outRoot, { recursive: true, force: true });

for (const [name, spec] of Object.entries(config)) {
  const api = `https://api.github.com/repos/${spec.repo}/releases/tags/${spec.tag}`;
  const res = await fetch(api, { headers });
  if (!res.ok) throw new Error(`${api}: HTTP ${res.status}`);
  const release = await res.json();
  const pattern = new RegExp(spec.asset);
  const asset = release.assets.find((a) => pattern.test(a.name));
  if (!asset) {
    throw new Error(`${spec.repo} ${spec.tag}: '${spec.asset}'에 맞는 파일 없음. 있는 파일:\n${release.assets.map((a) => a.name).join('\n')}`);
  }

  console.log(`${name}: ${asset.name} 내려받는 중 (${(asset.size / 1e6).toFixed(1)} MB)`);
  const bin = await fetch(asset.browser_download_url, { headers: { 'User-Agent': headers['User-Agent'] } });
  if (!bin.ok) throw new Error(`${asset.browser_download_url}: HTTP ${bin.status}`);
  const buf = Buffer.from(await bin.arrayBuffer());

  const sha256 = createHash('sha256').update(buf).digest('hex');
  if (asset.digest && asset.digest !== `sha256:${sha256}`) {
    throw new Error(`${asset.name}: 해시 불일치 (기대 ${asset.digest}, 실제 sha256:${sha256})`);
  }

  const work = mkdtempSync(join(tmpdir(), `engine-${name}-`));
  const zipPath = join(work, asset.name);
  writeFileSync(zipPath, buf);
  const extractDir = join(work, 'x');
  mkdirSync(extractDir);
  // 윈도우 10 이상에 기본 포함된 tar(bsdtar)는 zip을 풀 수 있다.
  execFileSync('tar', ['-xf', zipPath, '-C', extractDir], { stdio: 'inherit' });

  const main = findFile(extractDir, spec.main);
  if (!main) throw new Error(`${asset.name} 안에 ${spec.main}이 없음`);
  const srcDir = dirname(main);
  const dest = join(outRoot, name);
  mkdirSync(dest, { recursive: true });
  const kept = [];
  for (const entry of readdirSync(srcDir)) {
    if (entry.toLowerCase() === spec.main.toLowerCase() || entry.toLowerCase().endsWith('.dll')) {
      copyFileSync(join(srcDir, entry), join(dest, entry));
      kept.push(entry);
    }
  }
  rmSync(work, { recursive: true, force: true });

  versions[name] = { repo: spec.repo, tag: spec.tag, asset: asset.name, sha256, files: kept.sort() };
  console.log(`  → ${dest} (${kept.length}개 파일)`);
}

writeFileSync(join(outRoot, 'VERSIONS.json'), JSON.stringify(versions, null, 2) + '\n');
if (!existsSync(join(outRoot, 'llama', config.llama.main))) throw new Error('llama-bench 없음');
console.log('엔진 준비 완료');
