import { invoke } from '@tauri-apps/api/core';
import { curriculum } from '../curriculum';

export interface SupportItem {
  name: string;
  label: string;
  bytes: number;
}

export interface SupportResult {
  path: string;
  items: SupportItem[];
}

const curriculumDataVersion = () => curriculum.framework.dataVersion;

export const supportApi = {
  preview: () => invoke<SupportItem[]>('support_preview', { curriculumDataVersion: curriculumDataVersion() }),
  create: (path: string) => invoke<SupportResult>('support_create', { input: { path, curriculumDataVersion: curriculumDataVersion() } }),
};

/** `바슬즐-진단-YYYYMMDD-HHMM.zip` */
export function defaultFileName(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `바슬즐-진단-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}.zip`;
}
