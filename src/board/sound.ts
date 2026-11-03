// Web Audio 비프. 소리 끄기는 이 PC의 이 화면에만 기억한다.
const KEY = 'board.sound';

export function soundOn(): boolean {
  try {
    return localStorage.getItem(KEY) !== 'off';
  } catch {
    return true;
  }
}

export function setSoundOn(on: boolean) {
  try {
    localStorage.setItem(KEY, on ? 'on' : 'off');
  } catch {
    /* 저장할 수 없어도 화면은 동작한다 */
  }
}

let ctx: AudioContext | null = null;

/** `times`번 짧게 울린다. 소리가 꺼져 있거나 오디오를 쓸 수 없으면 아무것도 하지 않는다. */
export function beep(times = 1, freq = 880) {
  if (!soundOn()) return;
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
