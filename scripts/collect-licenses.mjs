// 직접 의존성(npm dependencies, Rust 직접 의존성)의 이름·버전·라이선스를 모아
// src/about/licenses.json 을 만든다. 네트워크를 쓰지 않는다.
// cargo metadata --offline 이 실패하면 기존 파일의 Rust 목록을 그대로 둔다.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUT = join(root, 'src/about/licenses.json');

const byName = (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);

function licenseOf(pkg) {
  if (typeof pkg.license === 'string') return pkg.license;
  if (pkg.license && typeof pkg.license.type === 'string') return pkg.license.type;
  if (Array.isArray(pkg.licenses)) return pkg.licenses.map((l) => l.type).join(' OR ');
  return '확인 필요';
}

export function collectNpm(rootDir = root) {
  const pkg = JSON.parse(readFileSync(join(rootDir, 'package.json'), 'utf8'));
  return Object.keys(pkg.dependencies ?? {})
    .map((name) => {
      const p = JSON.parse(readFileSync(join(rootDir, 'node_modules', name, 'package.json'), 'utf8'));
      return { name, version: p.version, license: licenseOf(p) };
    })
    .sort(byName);
}

export function collectRust(rootDir = root) {
  const manifest = join(rootDir, 'src-tauri/Cargo.toml');
  const out = execFileSync('cargo', ['metadata', '--format-version', '1', '--offline', '--manifest-path', manifest], {
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const meta = JSON.parse(out);
  const me = meta.packages.find((p) => p.name === 'tonghap-module');
  if (!me) throw new Error('tonghap-module 패키지를 찾을 수 없음');
  // 이름만으로 고르면 같은 크레이트의 다른 버전(간접 의존성)까지 섞이므로,
  // 해석된 의존성 그래프에서 이 패키지가 직접 쓰는 패키지 id만 고른다.
  const node = meta.resolve?.nodes?.find((n) => n.id === me.id);
  const directIds = new Set(
    (node?.deps ?? [])
      .filter((d) => (d.dep_kinds ?? [{ kind: null }]).some((k) => k.kind !== 'dev' && k.kind !== 'build'))
      .map((d) => d.pkg),
  );
  return meta.packages
    .filter((p) => directIds.has(p.id))
    .map((p) => ({ name: p.name, version: p.version, license: p.license ?? '확인 필요' }))
    .sort(byName);
}

function main() {
  let previous = { npm: [], rust: [] };
  if (existsSync(OUT)) previous = JSON.parse(readFileSync(OUT, 'utf8'));
  const npm = collectNpm();
  let rust;
  try {
    rust = collectRust();
  } catch (e) {
    console.warn(`cargo metadata 실패, 기존 Rust 목록을 유지합니다: ${String(e.message).split('\n')[0]}`);
    rust = previous.rust;
  }
  writeFileSync(OUT, JSON.stringify({ npm, rust }, null, 2) + '\n');
  console.log(`licenses.json: npm ${npm.length}개, rust ${rust.length}개`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
