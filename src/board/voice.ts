// 안내 음성 원본: 선생님 녹음(DB 설정, Blob 주소)이 있으면 그것, 없으면 앱에 들어 있는 기본 파일.
import { invoke } from '@tauri-apps/api/core';
import type { CueId } from './cues';
import cues from '../../data/voice/cues.json';
import timer1min from './voice/timer-1min.ogg?no-inline';
import timer30s from './voice/timer-30s.ogg?no-inline';
import timerEnd from './voice/timer-end.ogg?no-inline';

export const CUES = (cues as { cues: { id: CueId; text: string; when: string }[] }).cues;

const DEFAULTS: Record<CueId, string> = { 'timer-1min': timer1min, 'timer-30s': timer30s, 'timer-end': timerEnd };

/** 선생님 녹음의 Blob 주소 캐시. `null`은 녹음 없음. */
const cache = new Map<CueId, string | null>();

async function fetchTeacher(id: CueId): Promise<string | null> {
  const bytes = await invoke<number[] | null>('voice_get', { id });
  if (!bytes || bytes.length === 0) return null;
  return URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'audio/wav' }));
}

/** 선생님 녹음 주소(없으면 `null`). 읽기에 실패하면 기본 음성을 쓰도록 `null`. */
export async function teacherUrl(id: CueId): Promise<string | null> {
  if (cache.has(id)) return cache.get(id) ?? null;
  let url: string | null = null;
  try {
    url = await fetchTeacher(id);
  } catch {
    url = null;
  }
  cache.set(id, url);
  return url;
}

/** 재생할 주소: 선생님 녹음 우선, 없으면 기본 파일. */
export async function voiceUrl(id: CueId): Promise<string> {
  return (await teacherUrl(id)) ?? DEFAULTS[id];
}

function forget(id: CueId) {
  const old = cache.get(id);
  if (old) URL.revokeObjectURL(old);
  cache.delete(id);
}

export async function saveVoice(id: CueId, wav: Uint8Array): Promise<void> {
  await invoke('voice_set', { id, wav: Array.from(wav) });
  forget(id);
}

export async function clearVoice(id: CueId): Promise<void> {
  await invoke('voice_clear', { id });
  forget(id);
}
