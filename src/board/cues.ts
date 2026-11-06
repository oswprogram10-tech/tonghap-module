// 타이머 안내 음성 시점 계산. 화면·소리와 분리한 순수 함수라 node --test 로 그대로 검사한다.

export type CueId = 'timer-1min' | 'timer-30s' | 'timer-end';

/** 시간 중간에 나오는 안내(끝 안내는 타이머가 끝날 때 따로 낸다). 오래 남은 것부터. */
export const MID_CUES: { id: CueId; at: number; minTotal: number }[] = [
  { id: 'timer-1min', at: 60, minTotal: 120 },
  { id: 'timer-30s', at: 30, minTotal: 60 },
];

/** 이 전체 시간에서 쓰는 중간 안내. */
function eligible(total: number) {
  return MID_CUES.filter((c) => total >= c.minTotal);
}

/**
 * 이번 틱에 낼 안내. 남은 시간이 시점 이하로 내려왔고 아직 내지 않은 안내를 모두 `played`에 더해 돌려주고,
 * 소리는 그중 가장 늦은 것(가장 가까운 시점) 하나만 낸다(탭이 멈춰 있다 돌아와 한꺼번에 지나갔을 때 겹치지 않게).
 */
export function dueCue(remaining: number, total: number, played: ReadonlySet<CueId>): { cue: CueId | null; played: Set<CueId> } {
  const next = new Set(played);
  let cue: CueId | null = null;
  for (const c of eligible(total)) {
    if (remaining <= c.at && !next.has(c.id)) {
      next.add(c.id);
      cue = c.id;
    }
  }
  return { cue, played: next };
}

/**
 * 시작·다시 시작할 때: 이미 지난 시점은 소리 없이 낸 것으로 친다.
 * 일시정지 중에 지나간 것이 없으므로 보통은 그대로지만, 시간을 바꿔 다시 시작할 때 중복을 막는다.
 */
export function markPassed(remaining: number, total: number, played: ReadonlySet<CueId>): Set<CueId> {
  const next = new Set(played);
  for (const c of eligible(total)) if (remaining < c.at) next.add(c.id);
  return next;
}

export type SoundMode = 'voice' | 'beep' | 'off';
export const SOUND_MODES: { id: SoundMode; label: string }[] = [
  { id: 'voice', label: '안내 음성' },
  { id: 'beep', label: '삑 소리' },
  { id: 'off', label: '끔' },
];

/** 저장된 값을 방식으로 바꾼다. 예전 'off'는 「끔」, 예전 'on'과 값이 없으면 「안내 음성」. */
export function parseSoundMode(raw: string | null): SoundMode {
  return raw === 'off' || raw === 'beep' ? raw : 'voice';
}

/** 44바이트 머리말의 16kHz 모노 PCM16 WAV를 `maxSeconds`까지로 자른다(머리말 크기 값도 맞춘다). 짧으면 그대로. */
export function trimWav(wav: Uint8Array, maxSeconds: number, rate = 16000): Uint8Array {
  const maxData = Math.floor(maxSeconds * rate) * 2;
  if (wav.length <= 44 + maxData) return wav;
  const out = wav.slice(0, 44 + maxData);
  const v = new DataView(out.buffer);
  v.setUint32(4, 36 + maxData, true);
  v.setUint32(40, maxData, true);
  return out;
}
