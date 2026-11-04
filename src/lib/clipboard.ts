// 클립보드 복사는 모두 이 파일을 거친다(다른 곳에서 navigator.clipboard.writeText를 직접 부르지 않는다).
// 디버그 빌드에서만(Rust 명령 e2e_debug_hooks가 true) 화면 E2E가 읽을 수 있게 복사하려던 글을 window.__lastCopied에 남긴다.
// 릴리스 빌드에서는 명령이 false라 아무것도 남기지 않는다.
import { invoke, isTauri } from '@tauri-apps/api/core';

export interface ClipboardDeps {
  isTauri: () => boolean;
  debugHooks: () => Promise<boolean>;
  write: (text: string) => Promise<void>;
  record: (text: string) => void;
}

const defaultDeps: ClipboardDeps = {
  isTauri: () => isTauri(),
  debugHooks: () => invoke<boolean>('e2e_debug_hooks'),
  write: (text) => navigator.clipboard.writeText(text),
  record: (text) => {
    (window as unknown as { __lastCopied?: string }).__lastCopied = text;
  },
};

/** 글을 클립보드에 복사한다. 실패하면 던진다. */
export async function copyText(text: string, deps: ClipboardDeps = defaultDeps): Promise<void> {
  if (deps.isTauri()) {
    let hooks = false;
    try {
      hooks = await deps.debugHooks();
    } catch {
      hooks = false;
    }
    if (hooks) deps.record(text);
  }
  await deps.write(text);
}
