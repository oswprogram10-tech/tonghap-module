// 타이머 소리: 안내 음성 / 삑 소리 / 끔. 고른 방식은 이 PC의 이 화면에만 기억한다.
import { voiceUrl } from './voice';
import { parseSoundMode, type CueId, type SoundMode } from './cues';

export { SOUND_MODES, type SoundMode } from './cues';

const KEY = 'board.sound';

export function getSoundMode(): SoundMode {
  try {
    return parseSoundMode(localStorage.getItem(KEY));
  } catch {
    return 'voice';
  }
}

export function setSoundMode(mode: SoundMode) {
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* 저장할 수 없어도 화면은 동작한다 */
  }
}

let ctx: AudioContext | null = null;

/** `times`번 짧게 울린다. 오디오를 쓸 수 없으면 아무것도 하지 않는다. */
export function beep(times = 1, freq = 880) {
  try {
    ctx ??= new AudioContext();
    const c = ctx;
    void c.resume();
    for (let i = 0; i < times; i++) {
      const t = c.currentTime + i * 0.35;
      const osc = c.createOscillator();
      const gain = c.createGain();
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, t);
      gain.gain.exponentialRampToValueAtTime(0.25, t + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.22);
      osc.connect(gain).connect(c.destination);
      osc.start(t);
      osc.stop(t + 0.25);
    }
  } catch {
    /* 소리를 못 내도 괜찮다 */
  }
}

const BEEPS: Record<CueId, number> = { 'timer-1min': 1, 'timer-30s': 2, 'timer-end': 3 };

let current: HTMLAudioElement | null = null;

/** 안내 음성(선생님 녹음, 없으면 기본 파일)을 재생한다. 재생하지 못하면 `false`. */
export async function playVoice(id: CueId): Promise<boolean> {
  try {
    const url = await voiceUrl(id);
    current?.pause();
    const audio = new Audio(url);
    current = audio;
    await audio.play();
    return true;
  } catch {
    return false;
  }
}

/** 고른 소리 방식대로 안내를 낸다. 음성이 안 나오면 삑 소리로 대신한다. */
export async function playCue(id: CueId) {
  const mode = getSoundMode();
  if (mode === 'off') return;
  if (mode === 'voice' && (await playVoice(id))) return;
  beep(BEEPS[id]);
}
