// 앱 화면(프런트엔드)에 들어가는 npm 패키지(운영 의존성, 간접 포함)의 라이선스 전문을 Markdown 으로 만든다.
// `npm ls --omit=dev --all --json` 으로 트리를 얻고, 각 패키지의 LICENSE/LICENCE/COPYING 파일을 읽는다.
// 파일이 없으면 package.json 의 license 와 저작권자 정보로 대신한다. 네트워크를 쓰지 않는다.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, parse } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const LICENSE_FILE = /^(licen[cs]e|copying)([-._].*)?$/i;
const FENCE = '~~~~';

function licenseOf(pkg) {
  if (typeof pkg.license === 'string') return pkg.license;
  if (pkg.license && typeof pkg.license.type === 'string') return pkg.license.type;
  if (Array.isArray(pkg.licenses)) return pkg.licenses.map((l) => l.type).join(' OR ');
  return '확인 필요';
}

function authorOf(pkg) {
  const a = pkg.author;
  if (!a) return '';
  return typeof a === 'string' ? a : [a.name, a.email && `<${a.email}>`].filter(Boolean).join(' ');
}

// 부모 폴더에서 시작해 위로 올라가며 node_modules/<name> 을 찾는다(노드 해석 규칙).
function locate(name, fromDir, rootDir) {
  let dir = fromDir;
  for (;;) {
    const cand = join(dir, 'node_modules', name);
    if (existsSync(join(cand, 'package.json'))) return cand;
    const up = dirname(dir);
    if (up === dir || dir === parse(dir).root || dir.length < rootDir.length) return null;
    dir = up;
  }
}

function licenseTextOf(dir) {
  const files = readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENSE_FILE.test(e.name))
    .map((e) => e.name)
    .sort();
  return files.map((f) => readFileSync(join(dir, f), 'utf8').replace(/\r\n/g, '\n').replace(/^\n+/, '').trimEnd()).join('\n\n');
}

export function collectNpmLicenses(rootDir = root) {
  const tree = JSON.parse(
    execFileSync('npm', ['ls', '--omit=dev', '--all', '--json'], {
      cwd: rootDir,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
      shell: process.platform === 'win32',
      stdio: ['ignore', 'pipe', 'ignore'],
    }),
  );
  const found = new Map();
  const walk = (deps, fromDir) => {
    for (const [name, node] of Object.entries(deps ?? {})) {
      const dir = locate(name, fromDir, rootDir);
      if (!dir) throw new Error(`${name} 패키지 폴더를 찾을 수 없음 (npm ci 를 먼저 실행하세요)`);
      const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
      const key = `${pkg.name ?? name}@${pkg.version}`;
      if (!found.has(key)) {
        const text = licenseTextOf(dir);
        found.set(key, {
          name: pkg.name ?? name,
          version: pkg.version,
          license: licenseOf(pkg),
          author: authorOf(pkg),
          text: text || null,
        });
      }
      walk(node.dependencies, dir);
    }
  };
  walk(tree.dependencies, rootDir);
  return [...found.values()].sort((a, b) =>
    a.name === b.name ? (a.version < b.version ? -1 : 1) : a.name < b.name ? -1 : 1,
  );
}

export function renderNpmMarkdown(list) {
  const lines = [];
  const counts = new Map();
  for (const p of list) counts.set(p.license, (counts.get(p.license) ?? 0) + 1);
  lines.push('### 요약', '', '| 라이선스 | 패키지 수 |', '|---|---|');
  for (const [lic, n] of [...counts].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))) lines.push(`| ${lic} | ${n} |`);
  lines.push('');
  for (const p of list) {
    lines.push(`#### ${p.name} ${p.version}`, '', `라이선스: ${p.license}`, '');
    if (p.text) {
      if (p.text.includes(FENCE)) throw new Error(`${p.name}: 라이선스 본문에 ${FENCE} 가 들어 있음`);
      lines.push(`${FENCE}text`, p.text, FENCE, '');
    } else {
      lines.push(
        `라이선스 파일이 패키지에 들어 있지 않습니다(license file not included in package). package.json 기준: ${p.license}${p.author ? `, 저작권자/작성자: ${p.author}` : ''}`,
        '',
      );
    }
  }
  return lines.join('\n');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const list = collectNpmLicenses();
  process.stdout.write(renderNpmMarkdown(list));
}
