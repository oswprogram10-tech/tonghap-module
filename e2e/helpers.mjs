// 네이티브 흐름 E2E용 도구: 테스트 파일 만들기, 대화상자 큐, 학급 준비.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { By, Key, until } from 'selenium-webdriver';
import { WAIT, click, typeInto, waitCount, waitVisible, xButton } from './lib.mjs';

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};

/** 단색 그라데이션 PNG(RGB 8비트)를 만든다. */
export function makePng(file, width = 64, height = 48) {
  const row = 1 + width * 3;
  const raw = Buffer.alloc(row * height);
  for (let y = 0; y < height; y++) {
    raw[y * row] = 0; // 필터 없음
    for (let x = 0; x < width; x++) {
      const o = y * row + 1 + x * 3;
      raw[o] = (x * 255) / width;
      raw[o + 1] = (y * 255) / height;
      raw[o + 2] = 160;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // 비트 깊이
  ihdr[9] = 2; // RGB
  const png = Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
  fs.writeFileSync(file, png);
  return file;
}

/** PNG 시그니처를 확인하고 (너비, 높이)를 헤더에서 읽는다. */
export function readPngSize(file) {
  const head = fs.readFileSync(file).subarray(0, 24);
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  if (head.length < 24 || !head.subarray(0, 8).equals(sig)) throw new Error(`PNG 파일이 아닙니다: ${file}`);
  return { width: head.readUInt32BE(16), height: head.readUInt32BE(20) };
}

/** 앱의 디버그 전용 대화상자 큐(queue.json)에 결과를 하나 더한다. */
export function queueDialog(dir, kind, paths) {
  const file = path.join(dir, 'queue.json');
  const list = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : [];
  list.push({ kind, paths });
  fs.writeFileSync(file, JSON.stringify(list));
}

export function filesUnder(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? filesUnder(p) : [p];
  });
}

export const tab = (name) => `//nav[@aria-label='화면 선택']//button[normalize-space(.)='${name}']`;
export const STUDENT_CHIPS = "//*[@role='group'][@aria-label='학생 선택']//button";
export const CARDS = "//button[contains(@class,'rec-card')]";
export const EDIT_DIALOG = "//*[@role='dialog'][@aria-label='기록 수정']";

/** 학급을 만들고 명단을 붙여 넣는다(처음 화면에서). 끝나면 관찰 기록 양식이 보인다. */
export async function createClassWithRoster(driver, className, names) {
  await typeInto(driver, "//label[contains(normalize-space(.),'학급 이름')]//input", className);
  await click(driver, xButton('학급 만들기'));
  await typeInto(driver, "//textarea[@aria-label='명단 붙여넣기']", names.map((n, i) => `${i + 1} ${n}`).join('\n'));
  const apply = await waitVisible(driver, xButton('명단 반영'));
  await driver.wait(until.elementIsEnabled(apply), WAIT, '「명단 반영」이 켜지지 않음');
  await apply.click();
  await waitCount(driver, STUDENT_CHIPS, names.length);
}

export async function selectStudent(driver, name) {
  await click(driver, `${STUDENT_CHIPS}[contains(.,'${name}')]`);
  await driver.wait(
    async () => (await driver.findElement(By.xpath(`${STUDENT_CHIPS}[contains(.,'${name}')]`)).getAttribute('aria-pressed')) === 'true',
    WAIT,
    `${name} 학생이 선택되지 않음`,
  );
}

/** 「저장」을 누르고 기록 카드가 하나 늘어날 때까지 기다린다. */
export async function saveObservation(driver) {
  const before = (await driver.findElements(By.xpath(CARDS))).length;
  await click(driver, "//button[contains(@class,'rec-save')]");
  await waitCount(driver, CARDS, before + 1);
}

export async function writeMemo(driver, text) {
  await typeInto(driver, "//textarea[@aria-label='메모']", text);
}

export async function pressEnter(el) {
  await el.sendKeys(Key.ENTER);
}

/** 이미지가 실제로 그려졌는지(naturalWidth > 0). */
export async function waitImageLoaded(driver, xpath, timeout = WAIT) {
  const img = await waitVisible(driver, xpath, timeout);
  await driver.wait(async () => driver.executeScript('return arguments[0].complete && arguments[0].naturalWidth > 0', img), timeout, `이미지가 그려지지 않음: ${xpath}`);
  return img;
}
