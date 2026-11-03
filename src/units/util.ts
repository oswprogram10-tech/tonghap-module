import { curriculum } from '../curriculum';
import type { SessionKind, SessionStatus, StandardMode, Totals } from './api';

export const KIND_LABEL: Record<SessionKind, string> = { general: '일반', safety: '안전', play: '놀이', makingbook: '메이킹북' };
export const STATUS_LABEL: Record<SessionStatus, string> = { planned: '계획', done: '완료', skipped: '건너뜀' };
export const MODE_LABEL: Record<StandardMode, string> = { repeat: '반복형', focus: '집중형', emergent: '발현형' };
export const MODE_SHORT: Record<StandardMode, string> = { repeat: '반', focus: '집', emergent: '발' };

/** 1차시 = 40분. */
export const SESSION_MINUTES = 40;

/** 분을 차시로 환산(소수 첫째 자리). */
export function toSessions(minutes: number): number {
  return Math.round((minutes / SESSION_MINUTES) * 10) / 10;
}

export function formatMinutes(minutes: number): string {
  return `${minutes}분`;
}

export function toolName(toolId: string): string {
  if (toolId === 'free') return '자유형';
  return curriculum.unitDevelopmentTools.find((t) => t.id === toolId)?.name ?? toolId;
}

export interface Requirement {
  kind: 'safety' | 'play' | 'makingbook';
  label: string;
  have: number;
  need: number;
  /** 놀이는 "이상". */
  atLeast: boolean;
  met: boolean;
}

/** 주제책별 필수 편성(안전 더하기·놀이·메이킹북) 채움 상황. */
export function requirements(totals: Totals): Requirement[] {
  const req = curriculum.framework.requiredModulesPerTopicBook;
  const row = (kind: Requirement['kind'], label: string, need: number, atLeast: boolean): Requirement => {
    const have = toSessions(totals.byKind[kind]);
    return { kind, label, have, need, atLeast, met: have >= need };
  };
  return [
    row('safety', '안전', req.safetySessions, false),
    row('play', '놀이', req.playSessionsMin, true),
    row('makingbook', '메이킹북', req.makingBookSessions, false),
  ];
}

export function topicTotalSessions(topicBookId: string | null): number {
  return curriculum.topicBooks.find((t) => t.id === topicBookId)?.sessions ?? 48;
}

/** 오늘 날짜(`YYYY-MM-DD`, 로컬). */
export function todayString(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}
