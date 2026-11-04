// 화면 E2E 3부: 세 교과 통합 평어와 문의용 진단 파일(설계 19). 대화상자는 디버그 빌드의 큐 파일로 대신한다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { after, before, describe, test } from 'node:test';
import { By, until } from 'selenium-webdriver';
import {
  OUT_DIR,
  ROOT,
  WAIT,
  click,
  collectedErrors,
  installErrorHook,
  makeDataDir,
  saveFailureArtifacts,
  sleep,
  startSession,
  stopSession,
  waitCount,
  waitVisible,
  xButton,
} from './lib.mjs';
import { createClassWithRoster, queueDialog, saveObservation, selectStudent, tab, writeMemo } from './helpers.mjs';
import { readZipEntries } from './zip.mjs';

const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'));

const STUDENT = '김하늘';
const SUBJECTS = ['바른 생활', '슬기로운 생활', '즐거운 생활'];
const MEMOS = [
  '화단의 꽃잎 모양을 돋보기로 살펴보고 차이를 이야기함',
  '교실 약속을 스스로 지키며 주변을 깨끗이 정리함',
  '노래에 맞추어 몸으로 즐겁게 표현하며 활동에 참여함',
];
const TAG = '탐구';

const EVAL_STUDENT = "//section[contains(@class,'ev-student')]";
const COMBINED = "//section[contains(@class,'ev-combined')]";
const COMBINED_TEXT = `${COMBINED}//textarea[@aria-label='통합본 글']`;

describe('화면 E2E 3부(통합 평어·진단 파일)', { concurrency: false }, () => {
  let session;
  let driver;
  let dataDir;
  let dialogDir;
  let filesDir;
  let combinedText = '';

  before(
    async () => {
      fs.rmSync(path.join(OUT_DIR, 'eval'), { recursive: true, force: true });
      dataDir = makeDataDir();
      dialogDir = fs.mkdtempSync(path.join(path.dirname(dataDir), 'tonghap-e2e-dialog-'));
      filesDir = fs.mkdtempSync(path.join(path.dirname(dataDir), 'tonghap-e2e-files-'));
      session = await startSession(dataDir, { TONGHAP_E2E_DIALOG_DIR: dialogDir });
      driver = session.driver;
    },
    { timeout: 180_000 },
  );

  after(
    async () => {
      await stopSession(session);
      await sleep(500);
      for (const d of [dataDir, dialogDir, filesDir]) {
        try {
          if (d) fs.rmSync(d, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
        } catch {
          // 윈도우에서 아직 잠겨 있을 수 있음. 임시 폴더라 남겨 둬도 된다.
        }
      }
    },
    { timeout: 60_000 },
  );

  const scenario = (name, fn, timeout = 180_000) =>
    test(name, { timeout }, async () => {
      try {
        await fn();
      } catch (e) {
        await saveFailureArtifacts(driver, `eval-${name}`);
        throw e;
      }
    });

  const expectNoErrors = async () => {
    assert.deepEqual(await collectedErrors(driver), [], '콘솔 오류가 있었습니다');
  };

  const enabledButton = async (xpath, what) => {
    const el = await waitVisible(driver, xpath, WAIT, what);
    await driver.wait(until.elementIsEnabled(el), WAIT, `${what}이(가) 켜지지 않음`);
    return el;
  };

  const inputValue = async (xpath) => (await driver.findElement(By.xpath(xpath))).getAttribute('value');

  /** 주제책을 고르고(이미 골라져 있으면 그대로) 세 교과 성취기준 단추가 나타나기를 기다린다. */
  const ensureTopic = async () => {
    const select = "//label[contains(normalize-space(.),'주제책')]//select";
    const el = await waitVisible(driver, select);
    if ((await el.getAttribute('value')) === '') await click(driver, `${select}/option[2]`);
    await waitCount(driver, "//*[@aria-label='성취기준 선택']//button", 3);
  };

  scenario('0. 준비: 학급과 학생 1명', async () => {
    await waitVisible(driver, "//h1[normalize-space(.)='바슬즐 탐험대']");
    await installErrorHook(driver);
    await createClassWithRoster(driver, 'E2E평어반', [STUDENT]);
  });

  scenario('1. 교과마다 성취기준·태그를 붙인 관찰 기록을 3개씩 저장한다', async () => {
    for (const [i, memo] of MEMOS.entries()) {
      await selectStudent(driver, STUDENT);
      await ensureTopic();
      // 한 기록에 바·슬·즐 성취기준을 모두 붙이면 교과마다 근거가 한 개씩 늘어난다.
      for (const subject of SUBJECTS) {
        const chip = `//*[@aria-label='성취기준 선택']//button[contains(.,'${subject}')]`;
        await click(driver, chip);
        await driver.wait(async () => (await driver.findElement(By.xpath(chip)).getAttribute('aria-pressed')) === 'true', WAIT, `${subject} 성취기준이 선택되지 않음`);
      }
      await click(driver, `//*[@role='group'][@aria-label='활동 태그']//button[normalize-space(.)='${TAG}']`);
      await writeMemo(driver, memo);
      await saveObservation(driver);
      console.log(`[e2e] 관찰 기록 ${i + 1}/3 저장`);
    }
    await expectNoErrors();
  });

  scenario('2. 평어 탭: 기간을 정하고 교과마다 템플릿 초안을 만들어 확정한다', async () => {
    await click(driver, tab('평어'));
    await waitVisible(driver, "//h2[contains(.,'평어 (생활기록부 초안)')]");
    // 기간: 오늘이 들어 있는 해 전체(학기 기본값은 날짜에 따라 오늘이 빠질 수 있다).
    const year = new Date().getFullYear();
    const from = "//label[contains(normalize-space(.),'시작일')]//input";
    const to = "//label[contains(normalize-space(.),'끝일')]//input";
    await waitVisible(driver, from);
    for (const [xp, value] of [[from, `${year}-01-01`], [to, `${year}-12-31`]]) {
      await driver.executeScript(
        `const el = document.evaluate(arguments[0], document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue;
         Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, arguments[1]);
         el.dispatchEvent(new Event('input', { bubbles: true }));`,
        xp,
        value,
      );
    }
    await driver.wait(async () => (await inputValue(from)) === `${year}-01-01` && (await inputValue(to)) === `${year}-12-31`, WAIT, '기간이 정해지지 않음');

    const side = `//aside[@aria-label='학생 목록']//button[contains(.,'${STUDENT}')]`;
    for (const subject of SUBJECTS) {
      await click(driver, `//*[@role='group'][@aria-label='교과 선택']//button[normalize-space(.)='${subject}']`);
      await click(driver, side);
      const make = await enabledButton(`${EVAL_STUDENT}//button[contains(.,'초안 만들기')]`, `${subject} 「초안 만들기」`);
      await make.click();
      await waitVisible(driver, `${EVAL_STUDENT}//*[@role='status'][contains(.,'템플릿으로 초안을 만들었습니다')]`, WAIT, `${subject} 템플릿 초안 안내`);
      const confirm = await enabledButton(`${EVAL_STUDENT}//button[normalize-space(.)='확정']`, `${subject} 「확정」`);
      await confirm.click();
      await waitVisible(driver, `${EVAL_STUDENT}//span[contains(@class,'ev-status-confirmed')]`, WAIT, `${subject} 확정 표시`);
      console.log(`[e2e] ${subject} 평어 확정`);
    }
    await expectNoErrors();
  });

  /** 지금 화면(교과 평어)의 문장 글을 합쳐 돌려준다. */
  const draftText = async () => {
    const areas = await driver.findElements(By.xpath(`${EVAL_STUDENT}//textarea[contains(@aria-label,'번째 문장')]`));
    const list = [];
    for (const a of areas) list.push((await a.getAttribute('value')).trim());
    return list.filter(Boolean);
  };

  scenario('3. 세 교과 합치기: 통합본에 바 → 슬 → 즐 순서로 문장이 있다', async () => {
    const perSubject = [];
    for (const subject of SUBJECTS) {
      await click(driver, `//*[@role='group'][@aria-label='교과 선택']//button[normalize-space(.)='${subject}']`);
      await waitVisible(driver, `${EVAL_STUDENT}//span[contains(@class,'ev-status-confirmed')]`, WAIT, `${subject} 확정 표시`);
      const sentences = await draftText();
      assert.ok(sentences.length > 0, `${subject} 확정 문장이 화면에 없음`);
      perSubject.push(sentences);
    }
    const combine = await enabledButton(xButton('세 교과 합치기'), '「세 교과 합치기」');
    await combine.click();
    await waitVisible(driver, COMBINED_TEXT, WAIT, '통합본 글상자');
    await driver.wait(async () => (await inputValue(COMBINED_TEXT)).trim() !== '', WAIT, '통합본 글상자가 비어 있음');
    combinedText = (await inputValue(COMBINED_TEXT)).trim();
    let pos = 0;
    for (const [i, sentences] of perSubject.entries()) {
      const found = combinedText.indexOf(sentences[0], pos);
      assert.notEqual(found, -1, `통합본에서 ${SUBJECTS[i]} 첫 문장을 앞 교과 뒤에서 찾지 못함(바→슬→즐 순서 확인).\n문장: ${sentences[0]}\n통합본: ${combinedText}`);
      pos = found + sentences[0].length;
    }
    await expectNoErrors();
  });

  scenario('4. 통합본 확정 → 복사: 복사한 글이 통합본과 같다', async () => {
    const confirm = await enabledButton(`${COMBINED}//button[normalize-space(.)='확정']`, '통합본 「확정」');
    await confirm.click();
    await waitVisible(driver, `${COMBINED}//span[contains(@class,'ev-status-confirmed')]`, WAIT, '통합본 확정 표시');
    const copy = await enabledButton(`${COMBINED}//button[normalize-space(.)='복사']`, '통합본 「복사」');
    await copy.click();
    await waitVisible(driver, `${COMBINED}//*[@role='status'][contains(.,'복사했습니다')]`, WAIT, '「복사했습니다」 안내(클립보드 쓰기 성공)');
    // WebView2에서 clipboard.readText()는 권한 창을 띄울 수 있어 부르지 않는다. 디버그 빌드가 남기는 기록으로 확인한다.
    const copied = await driver.executeScript('return window.__lastCopied === undefined ? null : window.__lastCopied');
    assert.notEqual(copied, null, 'window.__lastCopied 가 없음: 디버그 빌드의 클립보드 기록 장치(e2e_debug_hooks)가 동작하지 않음');
    const norm = (s) => s.replace(/\r\n/g, '\n').trim();
    assert.equal(norm(copied), norm(combinedText), '복사한 글이 통합본과 다름');
    await expectNoErrors();
  });

  scenario('5. 학급 내보내기 「통합본(NEIS용)」: BOM과 머리 줄, 통합본 줄', async () => {
    const out = path.join(filesDir, 'neis.txt');
    await click(driver, `//*[@role='group'][@aria-label='내보내기 형식']//button[normalize-space(.)='통합본(NEIS용)']`);
    queueDialog(dialogDir, 'save', [out]);
    await click(driver, xButton('학급 확정본 내보내기 (.txt)'));
    await driver.wait(() => fs.existsSync(out) && fs.statSync(out).size > 0, WAIT, '내보낸 txt 파일이 만들어지지 않음');
    await waitVisible(driver, "//*[@role='status'][contains(.,'통합본 1건을 저장했습니다')]", WAIT, '내보내기 완료 안내');
    const bytes = fs.readFileSync(out);
    assert.deepEqual([...bytes.subarray(0, 3)], [0xef, 0xbb, 0xbf], 'UTF-8 BOM이 없음');
    const lines = bytes.subarray(3).toString('utf8').split(/\r?\n/);
    assert.equal(lines[0], '번호\t이름\t성취수준 및 특기사항', `첫 줄이 다름: ${lines[0]}`);
    assert.ok(lines[1]?.startsWith(`1\t${STUDENT}\t`), `둘째 줄이 학생 줄이 아님: ${lines[1]}`);
    assert.equal(lines[1].split('\t').slice(2).join('\t').trim(), combinedText, '둘째 줄의 글이 통합본과 다름');
    await expectNoErrors();
  });

  scenario('6. 문의용 진단 파일: 목록 확인 후 만들면 about.json과 로그만 들어 있다', async () => {
    const zipPath = path.join(filesDir, 'diag.zip');
    await click(driver, tab('정보'));
    await click(driver, xButton('문의용 진단 파일 만들기'));
    const items = "//ul[contains(@class,'support-list')]/li";
    await waitVisible(driver, items, WAIT, '진단 파일 목록');
    const listed = [];
    for (const li of await driver.findElements(By.xpath(`${items}/code`))) listed.push((await li.getText()).trim());
    console.log(`[e2e] 화면 목록: ${listed.join(', ')}`);
    assert.ok(listed.includes('about.json') && listed.includes('logs/app.log'), `목록에 about.json, logs/app.log 가 없음: ${listed.join(', ')}`);

    queueDialog(dialogDir, 'save', [zipPath]);
    await click(driver, xButton('만들기'));
    await waitVisible(driver, "//*[@role='status'][contains(.,'진단 파일을 만들었습니다')]", 60_000, '진단 파일 완료 안내');
    assert.ok(fs.existsSync(zipPath), `zip 파일이 없음: ${zipPath}`);

    const entries = readZipEntries(fs.readFileSync(zipPath));
    const names = entries.map((e) => e.name);
    console.log(`[e2e] zip 항목: ${names.join(', ')}`);
    assert.deepEqual([...names].sort(), [...listed].sort(), '화면 목록과 zip 항목이 다름');
    assert.ok(names.includes('about.json') && names.includes('logs/app.log'), `zip에 about.json, logs/app.log 가 없음: ${names.join(', ')}`);
    const unexpected = names.filter((n) => n !== 'about.json' && !/^logs\/app(\.\d+)?\.log$/.test(n) && !/^diag\/device-check-[\d-]+\.json$/.test(n));
    assert.deepEqual(unexpected, [], '진단 파일에 들어가면 안 되는 항목이 있음');
    if (!names.some((n) => n.startsWith('diag/'))) console.log('[e2e] 기기 점검 결과 없음: diag/ 항목 없음(예상대로)');

    const about = JSON.parse(entries.find((e) => e.name === 'about.json').data.toString('utf8'));
    assert.equal(about.appVersion, pkg.version, `about.json appVersion(${about.appVersion})이 package.json 버전(${pkg.version})과 다름`);
    assert.equal(about.counts.students, 1, `about.json 학생 수가 1이 아님: ${about.counts.students}`);
    assert.ok(!JSON.stringify(about).includes(STUDENT), 'about.json에 학생 이름이 있음');

    const log = entries.find((e) => e.name === 'logs/app.log').data.toString('utf8');
    assert.ok(log.includes('시작'), `app.log에 「시작」 줄이 없음:\n${log.slice(0, 500)}`);
    assert.ok(!log.includes(STUDENT), 'app.log에 학생 이름이 가려지지 않고 남아 있음');
    await expectNoErrors();
  });

  scenario('7. 오류 기록 위치: TONGHAP_DATA_DIR 아래 logs/app.log', async () => {
    const log = path.join(dataDir, 'logs', 'app.log');
    assert.ok(fs.existsSync(log), `오류 기록 파일이 없음: ${log}`);
    const text = fs.readFileSync(log, 'utf8');
    assert.ok(text.includes('시작'), `${log}에 「시작」 줄이 없음`);
    assert.ok(!text.includes(STUDENT), `${log}에 학생 이름이 남아 있음`);
  });
});
