// 화면 E2E 2부: 파일 대화상자·마이크가 필요한 흐름(설계 14). 대화상자는 디버그 빌드의 큐 파일로 대신한다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { after, before, describe, test } from 'node:test';
import { By } from 'selenium-webdriver';
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
} from './lib.mjs';
import {
  CARDS,
  EDIT_DIALOG,
  createClassWithRoster,
  filesUnder,
  makePng,
  queueDialog,
  readPngSize,
  saveObservation,
  selectStudent,
  tab,
  waitImageLoaded,
  writeMemo,
} from './helpers.mjs';

const STUDENTS = ['김하늘', '이바다', '박구름'];
const SAMPLE_WAV = path.join(ROOT, 'src-tauri', 'resources', 'samples', 'jfk.wav');
const PHOTO_MEMO = '사진 첨부 기록';
const VOICE_MEMO = '음성 메모 기록';
const LATE_MEMO = '복원 전에 추가한 기록';
const BACKUP_PASSWORD = 'e2e-password-123';

describe('화면 E2E 2부(대화상자·마이크)', { concurrency: false }, () => {
  let session;
  let driver;
  let dataDir;
  let dialogDir;
  let filesDir;

  before(
    async () => {
      fs.rmSync(path.join(OUT_DIR, 'native'), { recursive: true, force: true });
      dataDir = makeDataDir();
      dialogDir = fs.mkdtempSync(path.join(path.dirname(dataDir), 'tonghap-e2e-dialog-'));
      filesDir = fs.mkdtempSync(path.join(path.dirname(dataDir), 'tonghap-e2e-files-'));
      const env = {
        TONGHAP_E2E_DIALOG_DIR: dialogDir,
        // 가짜 마이크: 권한 창 없이 허용하고, 장치 대신 jfk.wav를 들려준다.
        TONGHAP_E2E_BROWSER_ARGS: `--use-fake-ui-for-media-stream --use-fake-device-for-media-stream --use-file-for-fake-audio-capture=${SAMPLE_WAV}`,
      };
      if (process.env.TONGHAP_E2E_STT_MODEL) env.TONGHAP_E2E_STT_MODEL = process.env.TONGHAP_E2E_STT_MODEL;
      session = await startSession(dataDir, env);
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
        await saveFailureArtifacts(driver, `native-${name}`);
        throw e;
      }
    });

  const expectNoErrors = async () => {
    assert.deepEqual(await collectedErrors(driver), [], '콘솔 오류가 있었습니다');
  };

  const openCard = async (memo) => {
    await click(driver, `${CARDS}[contains(.,'${memo}')]`);
    await waitVisible(driver, EDIT_DIALOG);
  };
  const closeEditDialog = async () => {
    await click(driver, `${EDIT_DIALOG}//button[normalize-space(.)='닫기']`);
    await waitCount(driver, EDIT_DIALOG, 0);
  };

  scenario('0. 준비: 학급과 명단', async () => {
    await waitVisible(driver, "//h1[normalize-space(.)='바슬즐 탐험대']");
    await waitText(driver, '먼저 학급을 만들고 학생 명단을 붙여 넣어 주세요.');
    await installErrorHook(driver);
    await createClassWithRoster(driver, 'E2E반', STUDENTS);
  });

  scenario('1. 사진 첨부: 대화상자 대체로 고른 사진이 asset 주소로 실제로 그려진다', async () => {
    const png = makePng(path.join(filesDir, 'photo.png'));
    queueDialog(dialogDir, 'open', [png]);
    await selectStudent(driver, STUDENTS[0]);
    await click(driver, xButton('사진 추가'));
    await waitVisible(driver, "//ul[@aria-label='추가할 사진']/li[contains(.,'photo.png')]");
    await writeMemo(driver, PHOTO_MEMO);
    await saveObservation(driver);
    await openCard(PHOTO_MEMO);
    const img = await waitImageLoaded(driver, `${EDIT_DIALOG}//img[@alt='관찰 사진']`);
    const src = await img.getAttribute('src');
    assert.match(src, /asset\.localhost|^asset:/, `asset 프로토콜 주소가 아님: ${src}`);
    assert.equal(await driver.executeScript('return arguments[0].naturalWidth', img), 64);
    await closeEditDialog();
    await expectNoErrors();
  });

  scenario(
    '2. 음성 메모: 가짜 마이크로 녹음하고 자막이 끝 상태에 닿는다',
    async () => {
      assert.ok(process.env.TONGHAP_E2E_STT_MODEL, 'TONGHAP_E2E_STT_MODEL(테스트용 whisper 모델 경로)이 필요합니다');
      await selectStudent(driver, STUDENTS[1]);
      await click(driver, "//div[contains(@class,'rec-recorder')]//button[contains(.,'음성 메모')]");
      const stop = await waitVisible(driver, "//div[contains(@class,'rec-recorder')]//button[contains(.,'정지')]");
      await driver.wait(async () => /\((\d+)초\)/.test(await stop.getText()) && Number(/\((\d+)초\)/.exec(await stop.getText())[1]) >= 3, WAIT, '녹음이 3초에 이르지 않음');
      await stop.click();
      await waitVisible(driver, "//ul[@aria-label='추가할 음성 메모']/li");
      await writeMemo(driver, VOICE_MEMO);
      await saveObservation(driver);
      await openCard(VOICE_MEMO);

      const stt = `${EDIT_DIALOG}//div[contains(@class,'stt')]`;
      await waitVisible(driver, stt);
      const deadline = Date.now() + 5 * 60_000;
      const startedWait = Date.now();
      let last = '';
      for (;;) {
        const el = await driver.findElement(By.xpath(stt));
        const status = await el.getAttribute('data-status');
        if (status !== last) {
          console.log(`[e2e] 자막 상태: ${status}`);
          last = status;
        }
        if (status === 'done') break;
        if (status === 'failed') {
          const text = (await el.getText()).replace(/\s+/g, ' ');
          assert.ok(text.includes('말소리를 찾지 못했습니다'), `자막 실패: ${text}`);
          break;
        }
        if (status === 'none' && Date.now() - startedWait > 90_000) assert.fail('자동 자막이 시작되지 않았습니다(상태가 계속 none)');
        if (Date.now() > deadline) assert.fail(`자막이 5분 안에 끝나지 않았습니다(마지막 상태: ${status})`);
        await sleep(1000);
      }
      await closeEditDialog();
      await expectNoErrors();
    },
    8 * 60_000,
  );

  scenario(
    '3. 백업, 기록 추가, 복원: 처음 상태로 돌아오고 첨부가 다시 보인다',
    async () => {
      await waitCount(driver, CARDS, 2);
      const backupFile = path.join(filesDir, 'e2e.tonghap');

      // 백업 만들기(저장 대화상자 대체)
      await click(driver, tab('백업'));
      await typeInto(driver, "//label[contains(normalize-space(.),'암호 (8자 이상)')]//input", BACKUP_PASSWORD);
      await typeInto(driver, "//label[normalize-space(.)='암호 확인']//input", BACKUP_PASSWORD);
      queueDialog(dialogDir, 'save', [backupFile]);
      await click(driver, xButton('백업 파일 저장'));
      const done = await waitVisible(driver, "//*[contains(@class,'backup-done')][contains(.,'백업을 마쳤습니다')]", 120_000);
      assert.match(await done.getText(), /기록 2 · 첨부 2/);
      assert.ok(fs.statSync(backupFile).size > 0, '백업 파일이 비어 있음');

      // 기록 하나 더
      await click(driver, tab('관찰 기록'));
      await selectStudent(driver, STUDENTS[2]);
      await writeMemo(driver, LATE_MEMO);
      await saveObservation(driver);
      await waitCount(driver, CARDS, 3);

      // 복원(파일 선택 대체, 확인 창은 자동 수락)
      await click(driver, tab('백업'));
      queueDialog(dialogDir, 'open', [backupFile]);
      await click(driver, xButton('백업 파일 선택'));
      await waitVisible(driver, "//p[contains(@class,'diag-hint')][contains(.,'e2e.tonghap')]");
      await typeInto(driver, "//label[contains(normalize-space(.),'백업 암호')]//input", BACKUP_PASSWORD);
      await click(driver, xButton('내용 확인'));
      const row = await waitVisible(driver, "//th[normalize-space(.)='기록 · 첨부']/following-sibling::td", 60_000);
      assert.match(await row.getText(), /2개 · 2개/);
      await driver.executeScript('window.confirm = () => true;');
      await click(driver, xButton('이 백업으로 복원'));
      await waitVisible(driver, "//*[contains(@class,'backup-done')][contains(.,'복원을 마쳤습니다')]", 180_000);

      const dataSub = path.join(dataDir, 'data');
      assert.ok(fs.readdirSync(dataSub).some((n) => n.startsWith('before-restore-')), 'before-restore-* 폴더가 없음');

      // 원래 상태: 기록 2개, 나중에 추가한 기록 없음, 첨부 다시 표시
      await click(driver, tab('관찰 기록'));
      await waitCount(driver, CARDS, 2);
      assert.equal(await count(driver, `${CARDS}[contains(.,'${LATE_MEMO}')]`), 0, '복원 뒤에도 나중에 추가한 기록이 남아 있음');
      await openCard(PHOTO_MEMO);
      await waitImageLoaded(driver, `${EDIT_DIALOG}//img[@alt='관찰 사진']`);
      await closeEditDialog();
      await openCard(VOICE_MEMO);
      const audio = await waitVisible(driver, `${EDIT_DIALOG}//audio`);
      assert.match(await audio.getAttribute('src'), /asset\.localhost|^asset:/);
      await closeEditDialog();
      assert.ok(filesUnder(path.join(dataSub, 'media')).length >= 2, '복원된 미디어 폴더에 첨부 파일이 없음');
      await expectNoErrors();
    },
    6 * 60_000,
  );

  scenario('4. 학부모 리포트: PNG로 내보내면 시그니처와 1240x1754 크기가 맞다', async () => {
    const out = path.join(filesDir, 'report.png');
    await click(driver, tab('포트폴리오'));
    await click(driver, `//aside[@aria-label='학생 목록']//button[contains(.,'${STUDENTS[0]}')]`);
    await click(driver, xButton('학부모 리포트'));
    await waitVisible(driver, "//*[@role='dialog'][@aria-label='학부모 리포트']");
    const exportBtn = await waitVisible(driver, xButton('이미지로 내보내기'));
    await driver.wait(async () => exportBtn.isEnabled(), WAIT, '내보내기 단추가 켜지지 않음');
    queueDialog(dialogDir, 'save', [out]);
    await exportBtn.click();
    await driver.wait(() => fs.existsSync(out) && fs.statSync(out).size > 0, 90_000, '리포트 PNG 파일이 만들어지지 않음');
    await waitText(driver, '이미지를 저장했습니다');
    assert.deepEqual(readPngSize(out), { width: 1240, height: 1754 });
    await click(driver, "//*[@role='dialog'][@aria-label='학부모 리포트']//button[normalize-space(.)='닫기']");
    await expectNoErrors();
  });

  scenario('5. 메이킹북: 틀 인쇄 미리보기가 열리고 인쇄 단추가 window.print를 부른다', async () => {
    await click(driver, tab('단원'));
    await click(driver, xButton('새 단원'));
    await click(driver, "(//*[@aria-label='주제책 선택']//button[@role='radio'])[2]");
    await click(driver, xButton('만들기'));
    await click(driver, `(${xButton('차시 추가')})[1]`);
    await click(driver, xButton('활동 카드에서 고르기'));
    await click(driver, "//*[@aria-label='카드 종류']//button[normalize-space(.)='메이킹북']");
    await click(driver, "(//button[normalize-space(.)='틀 인쇄'])[1]");
    const dlg = "//*[@role='dialog'][@aria-label='메이킹북 틀 인쇄 미리보기']";
    await waitVisible(driver, dlg);
    await waitVisible(driver, `${dlg}//*[contains(@class,'mb-sheet')]`);
    // 인쇄 대화상자를 띄우지 않도록 가짜로 바꾸고 호출 횟수만 센다.
    await driver.executeScript('window.__printCalls = 0; window.print = () => { window.__printCalls += 1; };');
    await click(driver, `${dlg}//button[normalize-space(.)='인쇄']`);
    await driver.wait(async () => (await driver.executeScript('return window.__printCalls')) === 1, WAIT, 'window.print가 불리지 않음');
    await click(driver, `${dlg}//button[normalize-space(.)='닫기']`);
    await waitCount(driver, dlg, 0);
    await expectNoErrors();
  });
});
