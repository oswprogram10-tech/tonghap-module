import { mediaUrl } from '../lib/media';
import { encodeWav16 } from './wav';

export const STT_RATE = 16000;
export const STT_MAX_SECONDS = 300;

/** 녹음 바이트(webm/opus 등)를 디코드해 16kHz 모노 PCM16 WAV로 바꾼다. whisper-cli는 WAV를 읽는다. */
export async function toWav16k(data: ArrayBuffer): Promise<Uint8Array> {
  const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  const ctx = new Ctx();
  let decoded: AudioBuffer;
  try {
    decoded = await ctx.decodeAudioData(data.slice(0));
  } catch {
    throw new Error('이 녹음 형식을 읽을 수 없습니다');
  } finally {
    void ctx.close();
  }
  if (decoded.duration > STT_MAX_SECONDS) throw new Error('녹음이 너무 깁니다(5분 이하)');
  // 출력 채널을 1개로 두면 스테레오는 자동으로 모노로 섞인다.
  const off = new OfflineAudioContext(1, Math.max(1, Math.ceil(decoded.duration * STT_RATE)), STT_RATE);
  const src = off.createBufferSource();
  src.buffer = decoded;
  src.connect(off.destination);
  src.start();
  const rendered = await off.startRendering();
  return encodeWav16(rendered.getChannelData(0), STT_RATE);
}

/** 저장된 첨부 파일(tgmedia 프로토콜, 첨부 id)을 읽어 WAV로 바꾼다. */
export async function attachmentToWav(id: string): Promise<Uint8Array> {
  const res = await fetch(mediaUrl(id));
  if (!res.ok) throw new Error('녹음 파일을 읽을 수 없습니다');
  return toWav16k(await res.arrayBuffer());
}
