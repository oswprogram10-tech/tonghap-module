import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { availableMonitors, primaryMonitor } from '@tauri-apps/api/window';

export const BOARD_LABEL = 'board';

/**
 * 칠판 창을 연다. 이미 열려 있으면 앞으로 가져온다.
 * 모니터가 둘 이상이면 주 모니터가 아닌 쪽에 전체 화면으로, 하나면 최대화한 일반 창으로 연다.
 */
export async function openBoardWindow(): Promise<void> {
  const existing = await WebviewWindow.getByLabel(BOARD_LABEL);
  if (existing) {
    await existing.unminimize();
    await existing.show();
    await existing.setFocus();
    return;
  }
  const [monitors, primary] = await Promise.all([availableMonitors(), primaryMonitor()]);
  const other = monitors.find((m) => !primary || m.position.x !== primary.position.x || m.position.y !== primary.position.y);
  const base = { url: 'index.html#/board', title: '바슬즐 칠판', minWidth: 900, minHeight: 600 };
  let win: WebviewWindow;
  if (monitors.length > 1 && other) {
    const { x, y } = other.position.toLogical(other.scaleFactor);
    win = new WebviewWindow(BOARD_LABEL, { ...base, x, y, fullscreen: true });
  } else {
    win = new WebviewWindow(BOARD_LABEL, { ...base, width: 1280, height: 800, maximized: true });
  }
  await new Promise<void>((resolve, reject) => {
    void win.once('tauri://created', () => resolve());
    void win.once('tauri://error', (e) => reject(new Error(String(e.payload))));
  });
}
