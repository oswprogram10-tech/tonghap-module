// 칠판 모드의 무작위 로직. 난수 함수를 주입받는 순수 함수라서 같은 시드로 결과를 재현할 수 있다.
// 이 파일은 import 없이 타입만 쓴다(enum·네임스페이스 금지): Node의 타입 제거 실행(`npm test`)이 그대로 읽는다.

/** [0, 1) 난수를 돌려주는 함수. */
export type Rng = () => number;

/** 시드로 재현 가능한 난수(mulberry32). 테스트와 미리보기용. */
export function seededRng(seed: number): Rng {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** 실제 화면에서 쓰는 난수: crypto.getRandomValues. */
export function cryptoRng(): Rng {
  const buf = new Uint32Array(1);
  return () => {
    crypto.getRandomValues(buf);
    return buf[0] / 4294967296;
  };
}

export function randInt(rng: Rng, n: number): number {
  return Math.min(n - 1, Math.floor(rng() * n));
}

/** Fisher–Yates. 원본은 바꾸지 않는다. */
export function shuffle<T>(items: readonly T[], rng: Rng): T[] {
  const a = items.slice();
  for (let i = a.length - 1; i > 0; i--) {
    const j = randInt(rng, i + 1);
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/** 빠진 학생을 뺀다. */
export function present<T extends { id: string }>(students: readonly T[], absentIds: readonly string[]): T[] {
  const out = new Set(absentIds);
  return students.filter((s) => !out.has(s.id));
}

/** 무작위로 고르게 나눈다. 팀 인원 차이는 최대 1. 사람보다 팀이 많으면 팀 수를 줄인다. */
export function splitTeams<T>(members: readonly T[], teamCount: number, rng: Rng): T[][] {
  const n = Math.max(1, Math.min(Math.floor(teamCount), members.length || 1));
  const teams: T[][] = Array.from({ length: n }, () => []);
  shuffle(members, rng).forEach((m, i) => teams[i % n].push(m));
  return teams;
}

/** 최근 몇 번의 뽑기를 "최근"으로 볼지, 그 학생의 가중치. */
export const RECENT_ROUNDS = 3;
export const RECENT_WEIGHT = 0.15;

/**
 * 술래 뽑기. `history`는 지난 뽑기 결과(오래된 것 먼저, 한 번에 여러 명 가능).
 * 최근 3번 안에 뽑힌 학생은 가중치가 낮아져 덜 뽑힌다. 한 번에 중복 없이 `count`명.
 */
export function pickWeighted(candidates: readonly string[], count: number, history: readonly (readonly string[])[], rng: Rng): string[] {
  const recent = new Set(history.slice(-RECENT_ROUNDS).flat());
  const pool = candidates.map((id) => ({ id, w: recent.has(id) ? RECENT_WEIGHT : 1 }));
  const picked: string[] = [];
  const k = Math.max(0, Math.min(count, pool.length));
  for (let n = 0; n < k; n++) {
    const total = pool.reduce((s, p) => s + p.w, 0);
    let r = rng() * total;
    let idx = pool.findIndex((p) => (r -= p.w) < 0);
    if (idx < 0) idx = pool.length - 1;
    picked.push(pool[idx].id);
    pool.splice(idx, 1);
  }
  return picked;
}

export interface PlayLike {
  id: string;
  place: string;
  minutes: number;
  topicBookIds: string[];
}

export interface PlayFilter {
  /** 'outdoor' | 'indoor' | 'all'. 'both'인 놀이는 실외·실내 어느 쪽에도 들어간다. */
  place: string;
  /** 켜면 현재 주제책에 어울리는 놀이만. 주제책이 없으면 걸러지지 않는다. */
  topicBookId: string | null;
  onlyTopic: boolean;
  /** 켜면 15분 이하만. */
  short: boolean;
}

export const SHORT_MINUTES = 15;

export function filterPlays<T extends PlayLike>(plays: readonly T[], f: PlayFilter): T[] {
  return plays.filter((p) => {
    if (f.place === 'outdoor' && p.place === 'indoor') return false;
    if (f.place === 'indoor' && p.place === 'outdoor') return false;
    if (f.onlyTopic && f.topicBookId && !p.topicBookIds.includes(f.topicBookId)) return false;
    if (f.short && p.minutes > SHORT_MINUTES) return false;
    return true;
  });
}

/** 같은 놀이가 연속으로 나오지 않게 하나 고른다(후보가 하나뿐이면 어쩔 수 없다). */
export function pickPlay<T extends { id: string }>(plays: readonly T[], lastId: string | null, rng: Rng): T | null {
  if (plays.length === 0) return null;
  const others = plays.filter((p) => p.id !== lastId);
  const pool = others.length > 0 ? others : plays;
  return pool[randInt(rng, pool.length)];
}

/** 초를 "m:ss"로. */
export function formatClock(totalSeconds: number): string {
  const s = Math.max(0, Math.ceil(totalSeconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}
