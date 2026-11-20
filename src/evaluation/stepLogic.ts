// 평어 단계 화면(설계 25)의 순수 계산. 화면 없이 `node --test`로 확인한다.
import type { Evidence, Sentence } from './api';

export type Step = 1 | 2 | 3 | 4;
export type DraftStatus = 'none' | 'draft' | 'confirmed';

export const WEAK_NONE = '근거가 없는 문장입니다. 근거를 붙이거나 빼 주세요.';
export const WEAK_SESSION_ONLY = '차시 기록만 있고 이 학생의 관찰 기록은 없습니다. 그 모습을 보셨다면 확인하고, 아니면 문장을 빼거나 고쳐 주세요.';
export const WEAK_NEGATIVE = '단점 근거입니다. 변화 가능성이 함께 쓰였는지 확인해 주세요.';

export interface WeakReason {
  kind: 'none' | 'sessionOnly' | 'negative';
  message: string;
}

/** 한 문장의 근거가 약한 이유. 약하지 않으면 null. 목록에 없는 id는 없는 것으로 센다. */
export function weakReason(evidenceIds: string[], byId: Map<string, Evidence>): WeakReason | null {
  const found = evidenceIds.map((id) => byId.get(id)).filter((e): e is Evidence => !!e);
  if (found.length === 0) return { kind: 'none', message: WEAK_NONE };
  if (found.some((e) => e.kind === 'observation' && e.negative)) return { kind: 'negative', message: WEAK_NEGATIVE };
  if (!found.some((e) => e.kind === 'observation')) return { kind: 'sessionOnly', message: WEAK_SESSION_ONLY };
  return null;
}

export interface ListItem {
  id: string;
  done: boolean;
}

/** 지금 학생 다음 순서부터 한 바퀴 돌며 처음 만나는 미확정 학생. 없으면 null(지금 학생은 빼고 센다). */
export function nextUnconfirmed(items: ListItem[], currentId: string | null): string | null {
  if (items.length === 0) return null;
  const at = currentId === null ? -1 : items.findIndex((x) => x.id === currentId);
  for (let k = 1; k <= items.length; k += 1) {
    const x = items[(at + k) % items.length];
    if (x.id !== currentId && !x.done) return x.id;
  }
  return null;
}

/** 첫 번째로 확인하지 않은 문장(0부터). 모두 확인했으면 null. */
export function firstUnreviewed(sentences: Pick<Sentence, 'reviewed'>[]): number | null {
  const i = sentences.findIndex((s) => !s.reviewed);
  return i < 0 ? null : i;
}

/** 학생을 열 때 시작할 단계. 초안 없음 → 1, 확정 → 4, 초안 있음 → 3(첫 미확인 문장, 모두 확인했으면 4). */
export function startStep(status: DraftStatus, sentences: Pick<Sentence, 'reviewed'>[]): { step: Step; index: number } {
  if (sentences.length === 0) return { step: 1, index: 0 };
  if (status === 'confirmed') return { step: 4, index: 0 };
  const i = firstUnreviewed(sentences);
  return i === null ? { step: 4, index: 0 } : { step: 3, index: i };
}

/** 「맞아요, 확인했어요」를 누른 뒤 갈 곳: 다음 문장 / 아직 확인 안 한 문장 / 모두 끝났으면 확정 단계. */
export function afterReview(sentences: Pick<Sentence, 'reviewed'>[], index: number): { step: Step; index: number } {
  const next = sentences.map((s, i) => (i === index ? { ...s, reviewed: true } : s));
  if (index < next.length - 1) return { step: 3, index: index + 1 };
  const i = firstUnreviewed(next);
  return i === null ? { step: 4, index } : { step: 3, index: i };
}

export interface ConfirmLabel {
  label: string;
  /** confirm: 확정 실행, goto: 그 문장으로 이동, blocked: 근거·경고 때문에 누를 수 없음, none: 문장이 없음 */
  action: 'confirm' | 'goto' | 'blocked' | 'none';
  index: number;
}

/** 확정 단추의 문구와 동작. 남은 확인이 있으면 「3번 문장 확인 필요」처럼 보이고, 누르면 그 문장으로 간다. */
export function confirmLabel(input: { count: number; firstUnreviewed: number | null; canConfirm: boolean; unit?: '문장' | '통합본' }): ConfirmLabel {
  const unit = input.unit ?? '문장';
  if (input.count === 0) return { label: '확정', action: 'none', index: 0 };
  if (input.firstUnreviewed !== null) {
    const label = unit === '통합본' ? '통합본 확인 필요' : `${input.firstUnreviewed + 1}번 문장 확인 필요`;
    return { label, action: 'goto', index: input.firstUnreviewed };
  }
  if (!input.canConfirm) return { label: '확정', action: 'blocked', index: 0 };
  return { label: '확정', action: 'confirm', index: 0 };
}

/** NEIS 바이트: 한글 3, ASCII 1, 줄바꿈 1, `\r`은 세지 않음, 그 밖의 글자 3(Rust `neis_bytes`와 같다). */
export function neisBytes(text: string): number {
  let n = 0;
  for (const c of text) {
    if (c === '\r') continue;
    n += c === '\n' || c.charCodeAt(0) < 128 ? 1 : 3;
  }
  return n;
}

/** 확인한 문장 수. */
export function reviewedCount(sentences: Pick<Sentence, 'reviewed'>[]): number {
  return sentences.filter((s) => s.reviewed).length;
}

/** 「2026-08-01」 → 「2026.8.1」 */
export function dotDate(d: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(d);
  return m ? `${m[1]}.${Number(m[2])}.${Number(m[3])}` : d;
}
