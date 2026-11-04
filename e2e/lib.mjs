// 화면 E2E 공용 도구: tauri-driver 실행, WebDriver 연결, 기다리기, 실패 스크린숏.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Builder, By, Capabilities, until } from 'selenium-webdriver';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const OUT_DIR = path.join(ROOT, 'e2e', 'out');
export const WAIT = 20_000;
const DRIVER_URL = 'http://127.0.0.1:4444/';

const exe = process.platform === 'win32' ? '.exe' : '';
export const config = {
  app: process.env.TONGHAP_E2E_APP ?? path.join(ROOT, 'src-tauri', 'target', 'debug', `tonghap-module${exe}`),
  tauriDriver: process.env.TAURI_DRIVER_PATH ?? 'tauri-driver',
  nativeDriver: process.env.MSEDGEDRIVER_PATH ?? '',
};

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** 이번 실행 전용 데이터 폴더(앱은 환경 변수 TONGHAP_DATA_DIR로 이 폴더를 쓴다). */
export function makeDataDir() {
  const base = fs.realpathSync.native(os.tmpdir());
  return fs.mkdtempSync(path.join(base, 'tonghap-e2e-'));
}

async function waitForDriver(child) {
  const until_ = Date.now() + 30_000;
  while (Date.now() < until_) {
    if (child.exitCode !== null) throw new Error(`tauri-driver가 바로 종료되었습니다(종료 코드 ${child.exitCode}).`);
    try {
      const res = await fetch(`${DRIVER_URL}status`);
      if (res.ok) return;
    } catch {
      // 아직 준비 안 됨
    }
    await sleep(250);
  }
  throw new Error('tauri-driver가 30초 안에 준비되지 않았습니다.');
}

// ---- 프로세스 정리: 실패해도 자식(tauri-driver, msedgedriver, 앱)이 남아 테스트가 끝나지 않는 일이 없게 한다 ----

const children = new Set();
const IMAGES = ['tonghap-module.exe', 'msedgedriver.exe', 'tauri-driver.exe'];

/** 동기적으로 프로세스 트리와 이름이 같은 잔여 프로세스를 모두 끈다(종료 직전에도 호출 가능). */
export function killAllSync() {
  if (process.platform !== 'win32') {
    for (const c of children) {
      try {
        c.kill('SIGKILL');
      } catch {
        // 이미 끝남
      }
    }
    return;
  }
  for (const c of children) {
    if (c.pid) spawnSync('taskkill', ['/PID', String(c.pid), '/T', '/F'], { stdio: 'ignore' });
  }
  for (const image of IMAGES) spawnSync('taskkill', ['/IM', image, '/T', '/F'], { stdio: 'ignore' });
}
process.on('exit', killAllSync);
for (const sig of ['SIGINT', 'SIGTERM', 'SIGBREAK']) {
  process.on(sig, () => {
    killAllSync();
    process.exit(1);
  });
}

function pipeWithPrefix(stream, prefix) {
  let rest = '';
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    const lines = (rest + chunk).split(/\r?\n/);
    rest = lines.pop() ?? '';
    for (const l of lines) console.log(`${prefix} ${l}`);
  });
  stream.on('end', () => {
    if (rest) console.log(`${prefix} ${rest}`);
  });
}

const withTimeout = (promise, ms, what) =>
  Promise.race([promise, new Promise((_, rej) => setTimeout(() => rej(new Error(`${what}이(가) ${ms / 1000}초 안에 끝나지 않았습니다.`)), ms).unref())]);

/** tauri-driver를 띄우고 앱을 실행한 WebDriver를 돌려준다. 실패하면 남은 프로세스를 모두 끄고 던진다. */
export async function startSession(dataDir, extraEnv = {}) {
  for (const [name, p] of [['앱 실행 파일(TONGHAP_E2E_APP)', config.app], ['msedgedriver(MSEDGEDRIVER_PATH)', config.nativeDriver]]) {
    if (!p || !fs.existsSync(p)) throw new Error(`${name}을(를) 찾을 수 없습니다: ${p || '(지정 안 됨)'}`);
  }
  // 환경 변수는 tauri-driver → msedgedriver → 앱으로 이어서 전달된다.
  const env = { ...process.env, TONGHAP_DATA_DIR: dataDir, ...extraEnv };
  const args = ['--native-driver', config.nativeDriver];
  console.log(`[e2e] 실행: ${config.tauriDriver} ${args.join(' ')}`);
  console.log(`[e2e] 앱: ${config.app}`);
  console.log(`[e2e] TONGHAP_DATA_DIR=${dataDir}`);
  const child = spawn(config.tauriDriver, args, { env, stdio: ['ignore', 'pipe', 'pipe'] });
  children.add(child);
  pipeWithPrefix(child.stdout, '[tauri-driver]');
  pipeWithPrefix(child.stderr, '[tauri-driver!]');
  child.on('error', (e) => console.error('tauri-driver 실행 실패:', e.message));
  const session = { driver: null, child };

  try {
    await waitForDriver(child);
    const caps = new Capabilities();
    caps.set('tauri:options', { application: config.app });
    caps.setBrowserName('wry');
    console.log('[e2e] 보낼 capabilities:', JSON.stringify([...caps.keys()].reduce((o, k) => ({ ...o, [k]: caps.get(k) }), {})));
    session.driver = await withTimeout(new Builder().usingServer(DRIVER_URL).withCapabilities(caps).build(), 120_000, '세션 만들기');
    await session.driver.manage().setTimeouts({ script: 30_000 });
    return session;
  } catch (e) {
    await stopSession(session);
    throw e;
  }
}

export async function stopSession(session) {
  if (!session) return;
  if (session.driver) {
    try {
      await withTimeout(session.driver.quit(), 15_000, 'driver.quit');
    } catch {
      // 이미 끝났거나 응답 없음: 아래에서 강제로 끈다
    }
  }
  killAllSync();
  children.delete(session.child);
  session.child.stdout?.destroy();
  session.child.stderr?.destroy();
}

/** 실패했을 때 모든 창의 스크린숏과 HTML을 e2e/out에 저장한다. */
export async function saveFailureArtifacts(driver, name) {
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const safe = name.replace(/[^\p{L}\p{N}_-]+/gu, '_');
  let handles = [];
  let original = null;
  try {
    original = await driver.getWindowHandle();
  } catch {
    // 현재 창이 없을 수 있음
  }
  try {
    handles = await driver.getAllWindowHandles();
  } catch (e) {
    console.error('창 목록을 읽을 수 없음:', e.message);
  }
  for (const [i, h] of handles.entries()) {
    try {
      await driver.switchTo().window(h);
      fs.writeFileSync(path.join(OUT_DIR, `${safe}-창${i + 1}.png`), await driver.takeScreenshot(), 'base64');
      fs.writeFileSync(path.join(OUT_DIR, `${safe}-창${i + 1}.html`), await driver.getPageSource());
    } catch (e) {
      console.error(`스크린숏 저장 실패(창 ${i + 1}):`, e.message);
    }
  }
  try {
    if (original && handles.includes(original)) await driver.switchTo().window(original);
  } catch {
    // 무시
  }
}

// ---- 요소 찾기·기다리기 ----

/** XPath 문자열 리터럴(따옴표 포함 가능). */
export function xq(s) {
  if (!s.includes("'")) return `'${s}'`;
  const parts = s.split("'").map((p) => `'${p}'`);
  return `concat(${parts.join(`, "'", `)})`;
}

export const xButton = (text) => `//button[normalize-space(.)=${xq(text)}]`;
export const xButtonHas = (text) => `//button[contains(normalize-space(.),${xq(text)})]`;
export const xH = (text) => `//*[self::h1 or self::h2 or self::h3][normalize-space(.)=${xq(text)}]`;

/** 보이는 요소가 나타날 때까지 기다려 돌려준다. */
export async function waitVisible(driver, xpath, timeout = WAIT, what = xpath) {
  const el = await driver.wait(until.elementLocated(By.xpath(xpath)), timeout, `찾지 못함: ${what}`);
  await driver.wait(until.elementIsVisible(el), timeout, `보이지 않음: ${what}`);
  return el;
}

export async function click(driver, xpath, timeout = WAIT) {
  const deadline = Date.now() + timeout;
  let last;
  while (Date.now() < deadline) {
    try {
      const el = await waitVisible(driver, xpath, Math.max(1000, deadline - Date.now()));
      await driver.executeScript('arguments[0].scrollIntoView({block:"center"})', el);
      await el.click();
      return;
    } catch (e) {
      // 다시 그려지는 중에 요소가 사라지는 경우(StaleElement, 가려짐)만 재시도
      if (!/stale|intercept|not interactable|not clickable/i.test(String(e?.name) + String(e?.message))) throw e;
      last = e;
      await sleep(150);
    }
  }
  throw last ?? new Error(`눌러지지 않음: ${xpath}`);
}

/** 입력칸을 비우고 글자를 넣는다. React가 값을 알아채도록 실제 키 입력을 쓴다. */
export async function typeInto(driver, xpath, text) {
  const el = await waitVisible(driver, xpath);
  await driver.executeScript('arguments[0].scrollIntoView({block:"center"})', el);
  await el.click();
  await el.clear();
  await el.sendKeys(text);
  return el;
}

export async function waitText(driver, text, timeout = WAIT) {
  await waitVisible(driver, `//*[contains(normalize-space(text()),${xq(text)})]`, timeout, `글자 「${text}」`);
}

export async function count(driver, xpath) {
  return (await driver.findElements(By.xpath(xpath))).length;
}

export async function waitCount(driver, xpath, n, timeout = WAIT) {
  await driver.wait(async () => (await count(driver, xpath)) === n, timeout, `${xpath} 개수가 ${n}이(가) 되지 않음`);
}

// ---- 콘솔 오류 수집 ----
// 브라우저 로그 API는 tauri-driver에서 쓸 수 있다는 보장이 없어서, 창 안에 감시 코드를 심는다.

const HOOK = `
if (!window.__e2eErrors) {
  window.__e2eErrors = [];
  const push = (kind, msg) => window.__e2eErrors.push(kind + ': ' + msg);
  const origError = console.error.bind(console);
  console.error = (...a) => { push('console.error', a.map((x) => (x && x.stack) || String(x)).join(' ')); origError(...a); };
  window.addEventListener('error', (e) => push('error', e.message));
  window.addEventListener('unhandledrejection', (e) => push('unhandledrejection', String(e.reason && e.reason.message || e.reason)));
  document.addEventListener('securitypolicyviolation', (e) => push('CSP', e.violatedDirective + ' ' + e.blockedURI));
}
return true;`;

export const installErrorHook = (driver) => driver.executeScript(HOOK);

/** 심어 둔 감시 코드에 모인 오류. 무시할 만한 메시지는 IGNORE로 거른다. */
const IGNORE = [/ResizeObserver loop/];
export async function collectedErrors(driver) {
  const list = (await driver.executeScript('return window.__e2eErrors || null')) ?? [];
  return list.filter((m) => !IGNORE.some((re) => re.test(m)));
}
