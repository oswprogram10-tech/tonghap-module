// 화면(프런트엔드) 오류를 앱의 오류 기록(logs/app.log)에 남긴다. 이 PC 안의 파일에만 쓰며 인터넷으로 보내지 않는다.
import { invoke, isTauri } from '@tauri-apps/api/core';

const MAX = 300;

function text(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e) ?? String(e);
  } catch {
    return String(e);
  }
}

/** 기록하다 실패해도 화면에는 영향을 주지 않는다. */
export function reportError(message: string, source = '', line?: number) {
  if (!isTauri()) return;
  invoke('log_frontend_error', {
    input: { message: message.slice(0, MAX), source: source.slice(0, MAX), line: line && line > 0 ? line : undefined },
  }).catch(() => {});
}

let installed = false;

/** `window.onerror`와 `unhandledrejection`을 기록에 연결한다(한 번만). */
export function installErrorLogging() {
  if (installed) return;
  installed = true;
  window.addEventListener('error', (ev) => reportError(ev.message || text(ev.error), ev.filename, ev.lineno));
  window.addEventListener('unhandledrejection', (ev) => reportError(`처리되지 않은 오류: ${text(ev.reason)}`));
}
