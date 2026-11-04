// 파일 대화상자는 모두 이 파일을 거친다(다른 곳에서 @tauri-apps/plugin-dialog를 직접 부르지 않는다).
// 디버그 빌드의 화면 E2E는 Rust 명령 e2e_dialog_next로 대화상자를 대신한다. 릴리스 빌드에서는 항상 null이라
// 진짜 대화상자가 열린다.
import { invoke, isTauri } from '@tauri-apps/api/core';
import { open, save, type OpenDialogOptions, type SaveDialogOptions } from '@tauri-apps/plugin-dialog';

type Kind = 'open' | 'save' | 'folder';

async function scripted(kind: Kind): Promise<string[] | null> {
  if (!isTauri()) return null;
  return invoke<string[] | null>('e2e_dialog_next', { kind });
}

/** 여러 파일 고르기. 취소하면 null. */
export async function pickFiles(options: Omit<OpenDialogOptions, 'multiple' | 'directory'> = {}): Promise<string[] | null> {
  const fake = await scripted('open');
  if (fake) return fake;
  const picked = await open({ ...options, multiple: true, directory: false });
  if (!picked) return null;
  return Array.isArray(picked) ? picked : [picked];
}

/** 파일 하나 고르기. 취소하면 null. */
export async function pickFile(options: Omit<OpenDialogOptions, 'multiple' | 'directory'> = {}): Promise<string | null> {
  const fake = await scripted('open');
  if (fake) return fake[0] ?? null;
  const picked = await open({ ...options, multiple: false, directory: false });
  return typeof picked === 'string' ? picked : null;
}

/** 폴더 고르기. 취소하면 null. */
export async function pickFolder(options: Omit<OpenDialogOptions, 'multiple' | 'directory'> = {}): Promise<string | null> {
  const fake = await scripted('folder');
  if (fake) return fake[0] ?? null;
  const picked = await open({ ...options, multiple: false, directory: true });
  return typeof picked === 'string' ? picked : null;
}

/** 저장할 파일 경로 고르기. 취소하면 null. */
export async function saveFile(options: SaveDialogOptions = {}): Promise<string | null> {
  const fake = await scripted('save');
  if (fake) return fake[0] ?? null;
  return (await save(options)) ?? null;
}
