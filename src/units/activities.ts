// src-tauri/src/db/activities.rs 의 명령과 1:1로 대응한다. 기본 카드는 빌드 때 JSON으로 읽는다.
import { invoke } from '@tauri-apps/api/core';
import builtin from '../../data/activities/activities.json';
import { curriculum } from '../curriculum';

export type ActivityKind = 'general' | 'safety' | 'makingbook';

export interface Activity {
  id: string;
  topicBookId: string;
  title: string;
  subject: '바' | '슬' | '즐';
  standards: string[];
  kind: ActivityKind;
  minutes: 40 | 80;
  steps: string[];
  materials: string[];
  keywords: string[];
}

export interface Recommendation {
  activity: Activity;
  score: number;
  reasons: string[];
}

export interface RecommendInput {
  unitId: string;
  afterSessionId?: string;
  questions?: string;
}

export const BUILTIN_ACTIVITIES = (builtin as { activities: Activity[] }).activities;

export const ACTIVITY_KIND_LABEL: Record<ActivityKind, string> = { general: '일반', safety: '안전', makingbook: '메이킹북' };

export const activitiesApi = {
  customList: () => invoke<Activity[]>('activity_custom_list'),
  customSave: (activity: Omit<Activity, 'id'> & { id?: string }) => invoke<Activity>('activity_custom_save', { activity }),
  customDelete: (id: string) => invoke<void>('activity_custom_delete', { id }),
  recommend: (input: RecommendInput) => invoke<Recommendation[]>('activity_recommend', { input }),
};

/** 차시 활동 내용에 넣는 글(Rust `plan_text`와 같은 모양): 진행 순서를 번호 목록으로, 준비물이 있으면 한 줄 더. */
export function cardPlan(card: Activity): string {
  const steps = card.steps.map((s, i) => `${i + 1}. ${s}`).join('\n');
  return card.materials.length > 0 ? `${steps}\n\n준비물: ${card.materials.join(', ')}` : steps;
}

/** 단원에서 고를 수 있는 카드: 주제책이 있으면 그 주제책의 카드, 없으면 학년의 모든 주제책. */
export function cardsForUnit(all: Activity[], topicBookId: string | null, grade: number): Activity[] {
  return all.filter((a) => (topicBookId ? a.topicBookId === topicBookId : a.topicBookId.startsWith(`g${grade}s`)));
}

export function subjectChip(code: string): { id: string; name: string } {
  const s = curriculum.subjects.find((x) => x.code === code);
  return { id: s?.id ?? 'bareun', name: s?.name ?? code };
}
