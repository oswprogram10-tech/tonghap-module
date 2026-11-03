// src-tauri/src/backup 의 명령과 1:1로 대응한다.
import { Channel, invoke } from '@tauri-apps/api/core';

export interface BackupCounts {
  classes: number;
  students: number;
  observations: number;
  attachments: number;
}

export type BackupStage = 'snapshot' | 'hashing' | 'writing' | 'reading' | 'verifying' | 'swapping';

export interface BackupProgress {
  stage: BackupStage;
  done: number;
  total: number;
}

export interface BackupResult {
  path: string;
  bytes: number;
  counts: BackupCounts;
  missing: string[];
}

export interface BackupSummary {
  formatVersion: number;
  appVersion: string;
  schemaVersion: number;
  createdAt: string;
  counts: BackupCounts;
  fileCount: number;
  totalBytes: number;
}

export interface BackupStatus {
  lastBackup: { at: string; path: string; observations: number } | null;
  observationsSince: number;
}

function channel(onProgress: (p: BackupProgress) => void) {
  const c = new Channel<BackupProgress>();
  c.onmessage = onProgress;
  return c;
}

export const backupApi = {
  create: (path: string, password: string, onProgress: (p: BackupProgress) => void) =>
    invoke<BackupResult>('backup_create', { path, password, onProgress: channel(onProgress) }),
  inspect: (path: string, password: string) => invoke<BackupSummary>('backup_inspect', { path, password }),
  restore: (path: string, password: string, onProgress: (p: BackupProgress) => void) =>
    invoke<BackupSummary>('backup_restore', { path, password, onProgress: channel(onProgress) }),
  status: () => invoke<BackupStatus>('backup_status'),
};

export const BACKUP_REMINDER_KEY = 'backupReminderDismissedOn';
export const REMINDER_DAYS = 7;

/** 오늘(로컬) 날짜 `YYYY-MM-DD`. */
export function todayKey(d = new Date()) {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function defaultBackupName(d = new Date()) {
  const p = (n: number) => String(n).padStart(2, '0');
  return `바슬즐-백업-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}.tonghap`;
}
