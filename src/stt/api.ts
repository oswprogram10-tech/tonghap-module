// src-tauri/src/stt.rs 의 명령과 1:1로 대응한다.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Attachment } from '../records/api';
import { attachmentToWav } from './convert';

export const STT_MODELS = [
  { id: 'whisper-small-q5', label: '기본', note: '정확도와 속도의 균형' },
  { id: 'whisper-base-q5', label: '느린 PC용', note: '더 빠르지만 덜 정확' },
] as const;

export interface SttStatus {
  modelId: string;
  modelReady: boolean;
  engineReady: boolean;
  auto: boolean;
  queueLength: number;
}

export interface SttUpdate {
  attachmentId: string;
  status: Attachment['transcriptStatus'];
}

export const MODEL_MISSING = '음성 자막 모델이 없습니다 — 기기 점검 탭에서 내려받기';

export const sttApi = {
  status: () => invoke<SttStatus>('stt_status'),
  setModel: (modelId: string) => invoke<void>('stt_set_model', { modelId }),
  setAuto: (auto: boolean) => invoke<void>('stt_set_auto', { auto }),
  /** 16kHz 모노 WAV를 원시 본문으로 보낸다. 대기열에 넣고 바로 돌아온다. */
  transcribe: (attachmentId: string, wav: Uint8Array) =>
    invoke<{ status: 'queued' }>('stt_transcribe', wav, { headers: { 'x-attachment-id': attachmentId } }),
  attachmentGet: (id: string) => invoke<Attachment>('attachment_get', { id }),
  setTranscript: (id: string, text: string) => invoke<Attachment>('attachment_set_transcript', { id, text }),
  /** 상태가 바뀔 때마다 부른다. 돌려받는 함수로 구독을 끝낸다. */
  onUpdated: async (cb: (u: SttUpdate) => void) => listen<SttUpdate>('stt://updated', (e) => cb(e.payload)),
};

/** 첨부를 WAV로 바꿔 자막 대기열에 넣는다. 모델이 없으면 MODEL_MISSING 오류. */
export async function requestTranscript(a: Pick<Attachment, 'id'>): Promise<void> {
  const st = await sttApi.status();
  if (!st.modelReady || !st.engineReady) throw new Error(MODEL_MISSING);
  await sttApi.transcribe(a.id, await attachmentToWav(a.id));
}

/** 저장 직후 자동 받아쓰기: 자동이 켜져 있고 모델이 준비돼 있을 때만, 한 건씩 차례로 요청한다. */
export async function autoTranscribe(list: Pick<Attachment, 'id'>[]): Promise<void> {
  if (list.length === 0) return;
  try {
    const st = await sttApi.status();
    if (!st.auto || !st.modelReady || !st.engineReady) return;
  } catch (e) {
    console.warn('자막 상태 확인 실패', e);
    return;
  }
  for (const a of list) {
    try {
      await sttApi.transcribe(a.id, await attachmentToWav(a.id));
    } catch (e) {
      console.warn('자동 자막 요청 실패', e);
    }
  }
}
