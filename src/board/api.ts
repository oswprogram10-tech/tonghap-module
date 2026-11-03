// src-tauri/src/db/plays.rs, habits.rs 의 명령과 1:1로 대응한다.
import { invoke } from '@tauri-apps/api/core';
import builtin from '../../data/plays/plays.json';

export type Place = 'outdoor' | 'indoor' | 'both';
export type Group = 'whole' | 'teams' | 'pairs' | 'small';

export interface Play {
  id: string;
  name: string;
  place: Place;
  group: Group;
  minutes: number;
  rules: string[];
  safety: string[];
  materials: string[];
  tags: string[];
  topicBookIds: string[];
}

export const PLACE_LABEL: Record<Place, string> = { outdoor: '실외', indoor: '실내', both: '실내·실외' };
export const GROUP_LABEL: Record<Group, string> = { whole: '학급 전체', teams: '팀 대항', pairs: '짝', small: '모둠' };
export const PLAY_TAGS = ['전래', '신체', '협동', '표현', '규칙', '조용한'] as const;

/** 앱에 들어 있는 기본 놀이(빌드 때 포함). */
export const BUILTIN_PLAYS = (builtin as { plays: Play[] }).plays;

export interface Habit {
  id: string;
  classId: string;
  title: string;
  icon: string;
  startsOn: string | null;
  endsOn: string | null;
  active: boolean;
  position: number;
  createdAt: string;
}

export interface HabitInput {
  id?: string;
  classId: string;
  title: string;
  icon: string;
  startsOn?: string | null;
  endsOn?: string | null;
  active: boolean;
}

export interface HabitEntry {
  studentId: string;
  number: number;
  name: string;
  done: boolean;
}

export interface HabitDay {
  habit: Habit;
  date: string;
  entries: HabitEntry[];
  doneCount: number;
  total: number;
}

export interface HabitWeek {
  dates: string[];
  rows: { studentId: string; number: number; name: string; marks: (boolean | null)[]; doneDays: number }[];
}

export interface HabitSummary {
  habitId: string;
  title: string;
  icon: string;
  doneDays: number;
  schoolDays: number;
}

export const boardApi = {
  customPlays: () => invoke<Play[]>('play_custom_list'),
  customPlaySave: (play: Omit<Play, 'id'> & { id?: string }) => invoke<Play>('play_custom_save', { play }),
  customPlayDelete: (id: string) => invoke<void>('play_custom_delete', { id }),
  playLog: (classId: string, playId: string, minutes?: number) => invoke<void>('play_log_add', { classId, playId, minutes: minutes ?? null }),
  habitList: (classId: string, includeInactive: boolean) => invoke<Habit[]>('habit_list', { classId, includeInactive }),
  habitSave: (input: HabitInput) => invoke<Habit>('habit_save', { input }),
  habitDelete: (id: string) => invoke<void>('habit_delete', { id }),
  habitDay: (habitId: string, date: string) => invoke<HabitDay>('habit_day', { habitId, date }),
  habitToggle: (habitId: string, studentId: string, date: string) =>
    invoke<{ done: boolean; doneCount: number; total: number }>('habit_toggle', { habitId, studentId, date }),
  habitWeek: (habitId: string, endDate: string) => invoke<HabitWeek>('habit_week', { habitId, endDate }),
  habitSummary: (studentId: string, endDate: string, days: number) =>
    invoke<HabitSummary[]>('habit_student_summary', { studentId, endDate, days }),
};

/** 로컬 날짜 'YYYY-MM-DD'. */
export function dateStr(d: Date = new Date()): string {
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${m}-${day}`;
}

export function addDays(date: string, delta: number): string {
  const [y, m, d] = date.split('-').map(Number);
  return dateStr(new Date(y, m - 1, d + delta));
}

const WEEKDAY = ['일', '월', '화', '수', '목', '금', '토'];
export function shortDate(date: string): string {
  const [y, m, d] = date.split('-').map(Number);
  return `${m}/${d}(${WEEKDAY[new Date(y, m - 1, d).getDay()]})`;
}

export const BOARD_CLASS_EVENT = 'board://class-changed';
