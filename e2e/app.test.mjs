// 화면 E2E: 실제 앱(WebView2)을 tauri-driver로 띄워 주요 흐름을 누른다.
// 실행: npm run e2e (환경 변수 TONGHAP_E2E_APP, TAURI_DRIVER_PATH, MSEDGEDRIVER_PATH는 lib.mjs의 config 참고)
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { after, before, describe, test } from 'node:test';
import { By, Key, until } from 'selenium-webdriver';
import {
  OUT_DIR,
  ROOT,
  WAIT,
  click,
  collectedErrors,
  count,
  installErrorHook,
  makeDataDir,
  saveFailureArtifacts,
  sleep,
  startSession,
  stopSession,
  typeInto,
  waitCount,
  waitText,
  waitVisible,
  xButton,
  xH,
} from './lib.mjs';

const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'));

const CLASS_NAME = 'E2E반';
const STUDENTS = ['김하늘', '이바다', '박구름'];
const MEMO = '꽃잎 책갈피 만들기 성공';
const TAG = '탐구';

const tab = (name) => `//nav[@aria-label='화면 선택']//button[normalize-space(.)='${name}']`;

describe('화면 E2E', { concurrency: false }, () => {
  let session;
  let driver;
  let dataDir;
  let mainHandle;

  before(async () => {
    fs.rmSync(OUT_DIR, { recursive: true, force: true });
    dataDir = makeDataDir();
    session = await startSession(dataDir);
    driver = session.driver;
    mainHandle = await driver.getWindowHandle();
  });

  after(async () => {
    await stopSession(session);
    await sleep(500);
    try {
      fs.rmSync(dataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
    } catch {
      // 윈도우에서 아직 잠겨 있을 수 있음. 임시 폴더라 남겨 둬도 된다.
    }
  });

  /** 시나리오 하나. 실패하면 모든 창의 스크린숏을 e2e/out에 저장한다. */
  const scenario = (name, fn) =>
    test(name, { timeout: 180_000 }, async () => {
      try {
        await fn();
      } catch (e) {
        await saveFailureArtifacts(driver, name);
        throw e;
      }
    });

  const expectNoErrors = async (d = driver, where = '메인 창') => {
    const errs = await collectedErrors(d);
    assert.deepEqual(errs, [], `${where}에서 콘솔 오류가 있었습니다`);
  };

  scenario('1. 앱이 뜨고 「관찰 기록」 탭이 보인다', async () => {
    await waitVisible(driver, "//h1[normalize-space(.)='바슬즐 탐험대']");
    const active = await waitVisible(driver, "//nav[@aria-label='화면 선택']//button[contains(@class,'active')]");
    assert.equal((await active.getText()).trim(), '관찰 기록');
    // 처음 화면에서 학급이 없으면 학급 만들기 안내가 나온다(= IPC로 DB를 읽었다는 뜻).
    await waitText(driver, '먼저 학급을 만들고 학생 명단을 붙여 넣어 주세요.');
    await installErrorHook(driver);
    // 가능하면 브라우저 로그(지원하지 않는 드라이버는 건너뜀)
    try {
      const logs = await driver.manage().logs().get('browser');
      const severe = logs.filter((l) => String(l.level) === 'SEVERE').map((l) => l.message);
      assert.deepEqual(severe, [], '브라우저 로그에 오류가 있습니다');
    } catch (e) {
      if (e instanceof assert.AssertionError) throw e;
    }
    await expectNoErrors();
  });

  scenario('2. 학급 만들기, 명단 붙여넣기, 관찰 기록 저장, 기록 찾기', async () => {
    await typeInto(driver, "//label[contains(normalize-space(.),'학급 이름')]//input", CLASS_NAME);
    await click(driver, xButton('학급 만들기'));

    const roster = await typeInto(driver, "//textarea[@aria-label='명단 붙여넣기']", STUDENTS.map((n, i) => `${i + 1} ${n}`).join('\n'));
    assert.ok(roster);
    const apply = await waitVisible(driver, xButton('명단 반영'));
    await driver.wait(until.elementIsEnabled(apply), WAIT, '「명단 반영」이 켜지지 않음');
    await apply.click();

    // 명단이 반영되면 관찰 기록 양식이 나타나고 학생 단추가 3개 생긴다.
    const chips = "//*[@role='group'][@aria-label='학생 선택']//button";
    await waitCount(driver, chips, STUDENTS.length);

    await click(driver, `${chips}[contains(.,'${STUDENTS[0]}')]`);
    await driver.wait(
      async () => (await driver.findElement(By.xpath(`${chips}[contains(.,'${STUDENTS[0]}')]`)).getAttribute('aria-pressed')) === 'true',
      WAIT,
      '학생이 선택되지 않음',
    );
    await click(driver, `//*[@role='group'][@aria-label='활동 태그']//button[normalize-space(.)='${TAG}']`);
    await typeInto(driver, "//textarea[@aria-label='메모']", MEMO);
    await click(driver, "//button[contains(@class,'rec-save')]");
    await waitVisible(driver, "//*[@role='status'][contains(.,'저장했습니다.')]");

    // 메모 낱말로 찾기
    const search = await typeInto(driver, "//input[@aria-label='기록 검색어']", '책갈피');
    await search.sendKeys(Key.ENTER);
    await waitVisible(driver, "//*[@role='status'][contains(.,'검색 결과 1건')]");
    const card = await waitVisible(driver, "//button[contains(@class,'rec-card')][contains(.,'책갈피')]");
    assert.match(await card.getText(), new RegExp(STUDENTS[0]));
    await expectNoErrors();
  });

  scenario('3. 포트폴리오에서 학생 타임라인에 기록이 보인다', async () => {
    await click(driver, tab('포트폴리오'));
    await click(driver, `//aside[@aria-label='학생 목록']//button[contains(.,'${STUDENTS[0]}')]`);
    await waitVisible(driver, `//h2[contains(.,'${STUDENTS[0]}')]`);
    await waitVisible(driver, xH('타임라인'));
    const entry = await waitVisible(driver, "//article[contains(@class,'rec-entry')][contains(.,'책갈피')]");
    assert.match(await entry.getText(), new RegExp(TAG));
    await expectNoErrors();
  });

  scenario('4. 새 단원(주제책 + 도구)을 만들고 활동 카드로 차시를 추가한다', async () => {
    await click(driver, tab('단원'));
    await click(driver, xButton('새 단원'));
    // 첫 번째 주제책(0번째 단추는 「주제책 없음」)
    await click(driver, "(//*[@aria-label='주제책 선택']//button[@role='radio'])[2]");
    await driver.wait(
      async () => (await driver.findElement(By.xpath("(//*[@aria-label='주제책 선택']//button[@role='radio'])[2]")).getAttribute('aria-checked')) === 'true',
      WAIT,
      '주제책이 선택되지 않음',
    );
    // 도구는 처음에 첫 번째 도구가 골라져 있다.
    await waitVisible(driver, "//*[@aria-label='단원 개발 도구 선택']//button[@role='radio' and @aria-checked='true']");
    await click(driver, xButton('만들기'));

    // 단원 화면: 첫 단계의 「차시 추가」
    await click(driver, `(${xButton('차시 추가')})[1]`);
    await click(driver, xButton('활동 카드에서 고르기'));
    await click(driver, "(//ul[contains(@class,'act-list')]//button[contains(@aria-label,'이 카드로 차시 만들기')])[1]");
    await waitVisible(driver, "//*[@role='dialog']//p[contains(.,'로 채웠습니다')]");
    await click(driver, "//*[@role='dialog']//button[contains(@class,'rec-save')]");
    await waitCount(driver, "//*[@role='dialog']", 0);
    await waitCount(driver, "//ol[contains(@class,'unit-sessions')]/li", 1);
    await expectNoErrors();
  });

  scenario('5. 칠판 모드: 두 번째 창이 열리고 타이머 탭이 보이며 닫을 수 있다', async () => {
    await driver.switchTo().window(mainHandle);
    assert.equal((await driver.getAllWindowHandles()).length, 1, '시작할 때 창이 하나여야 합니다');
    await click(driver, "//button[contains(@class,'board-open')]");
    await driver.wait(async () => (await driver.getAllWindowHandles()).length === 2, WAIT, '칠판 창이 열리지 않음');
    const boardHandle = (await driver.getAllWindowHandles()).find((h) => h !== mainHandle);
    assert.ok(boardHandle);
    await driver.switchTo().window(boardHandle);
    try {
      await waitVisible(driver, "//h1[normalize-space(.)='바슬즐 칠판']");
      await installErrorHook(driver);
      await click(driver, "//nav[@aria-label='칠판 메뉴']//button[normalize-space(.)='타이머']");
      await waitVisible(driver, "//*[@role='timer']");
      await expectNoErrors(driver, '칠판 창');
    } finally {
      await driver.close();
    }
    await driver.wait(async () => (await driver.getAllWindowHandles()).length === 1, WAIT, '칠판 창이 닫히지 않음');
    await driver.switchTo().window(mainHandle);
    await waitVisible(driver, "//h1[normalize-space(.)='바슬즐 탐험대']");
  });

  scenario('6. 정보 탭: 버전과 데이터 폴더', async () => {
    await click(driver, tab('정보'));
    const version = await waitVisible(driver, "//dt[normalize-space(.)='버전']/following-sibling::dd[1]");
    await driver.wait(async () => (await version.getText()).startsWith(`v${pkg.version}`), WAIT, `버전이 v${pkg.version}이(가) 아님`);

    const code = await waitVisible(driver, "//p[contains(.,'데이터 폴더')]//code");
    const shown = (await code.getText()).replaceAll('\\', '/').toLowerCase();
    const expected = path.join(dataDir, 'data').replaceAll('\\', '/').toLowerCase();
    assert.ok(shown.endsWith(expected) || shown === expected || shown.endsWith(`${path.basename(dataDir).toLowerCase()}/data`), `데이터 폴더가 임시 폴더 아래가 아님: ${shown} (기대: ${expected})`);
    assert.ok(fs.existsSync(path.join(dataDir, 'data', 'tonghap.db')), 'TONGHAP_DATA_DIR 아래에 DB 파일이 없음');
    await expectNoErrors();
  });

  scenario('7. 기기 점검: 시스템 정보 표가 채워진다', async () => {
    await click(driver, tab('기기 점검'));
    const rows = "//table[contains(@class,'sys-table')]//tr";
    await driver.wait(async () => (await count(driver, rows)) >= 7, WAIT, '시스템 정보 표가 채워지지 않음');
    const cell = (label) => `//table[contains(@class,'sys-table')]//th[normalize-space(.)='${label}']/following-sibling::td`;
    // 엔진 확인이 끝날 때까지(엔진 폴더가 없으면 「없음」으로 표시)
    const engines = await waitVisible(driver, cell('측정 엔진'));
    await driver.wait(async () => !(await engines.getText()).includes('확인하는 중'), WAIT, '측정 엔진 확인이 끝나지 않음');
    assert.equal((await (await waitVisible(driver, cell('모델 폴더 쓰기'))).getText()).trim(), '가능');
    // 러너에는 Vulkan이 없을 수 있으므로 「사용 불가」도 통과
    assert.match(await (await waitVisible(driver, cell('GPU (Vulkan)'))).getText(), /Vulkan|사용 불가/);
    assert.notEqual((await (await waitVisible(driver, cell('WebView2'))).getText()).trim(), '');
    assert.ok(fs.existsSync(path.join(dataDir, 'models')), '모델 폴더가 TONGHAP_DATA_DIR 아래에 만들어지지 않음');
    await expectNoErrors();
  });
});
