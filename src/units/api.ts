// src-tauri/src/db/units.rs 의 명령과 1:1로 대응한다. SQL은 Rust에만 있다.
import { invoke } from '@tauri-apps/api/core';

export type SessionKind = 'general' | 'safety' | 'play' | 'makingbook';
export type SessionStatus = 'planned' | 'done' | 'skipped';
export type StandardMode = 'repeat' | 'focus' | 'emergent';

export interface SessionStandard {
  code: string;
  mode: StandardMode;
}

export interface SessionInput {
  unitId: string;
  stageId: string;
  title: string;
  plan: string;
  minutes: number;
  kind: SessionKind;
  scheduledOn?: string | null;
  status: SessionStatus;
  studentQuestions: string;
  reflection: string;
  standards: SessionStandard[];
}

export interface Session extends SessionInput {
  id: string;
  position: number;
  followsSessionId: string | null;
  observationCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface Totals {
  sessions: number;
  doneSessions: number;
  minutes: number;
  doneMinutes: number;
  byKind: Record<SessionKind, number>;
  standards: { code: string; repeat: number; focus: number; emergent: number }[];
}

export interface Stage {
  id: string;
  position: number;
  title: string;
  sessions: Session[];
}

export interface UnitDetail {
  id: string;
  classId: string;
  topicBookId: string | null;
  toolId: string;
  title: string;
  memo: string;
  createdAt: string;
  updatedAt: string;
  archivedAt: string | null;
  stages: Stage[];
  totals: Totals;
}

export interface UnitSummary {
  id: string;
  title: string;
  topicBookId: string | null;
  toolId: string;
  updatedAt: string;
  archivedAt: string | null;
  totals: Totals;
}

export interface UnitCreateInput {
  classId: string;
  topicBookId?: string | null;
  toolId: string;
  title: string;
  stageTitles: string[];
}

export interface SessionOption {
  sessionId: string;
  unitId: string;
  unitTitle: string;
  topicBookId: string | null;
  sessionTitle: string;
  scheduledOn: string | null;
  status: SessionStatus;
  standardCodes: string[];
}

export const unitsApi = {
  list: (classId: string, includeArchived: boolean) => invoke<UnitSummary[]>('unit_list', { classId, includeArchived }),
  create: (input: UnitCreateInput) => invoke<UnitDetail>('unit_create', { input }),
  update: (id: string, title: string, memo: string, topicBookId: string | null) =>
    invoke<UnitDetail>('unit_update', { id, title, memo, topicBookId }),
  archive: (id: string) => invoke<void>('unit_archive', { id }),
  unarchive: (id: string) => invoke<void>('unit_unarchive', { id }),
  remove: (id: string) => invoke<void>('unit_delete', { id }),
  get: (id: string) => invoke<UnitDetail>('unit_get', { id }),
  stageRename: (id: string, title: string) => invoke<void>('stage_rename', { id, title }),
  sessionCreate: (input: SessionInput) => invoke<Session>('session_create', { input }),
  sessionUpdate: (id: string, input: SessionInput) => invoke<Session>('session_update', { id, input }),
  sessionMove: (id: string, stageId: string, position: number) => invoke<UnitDetail>('session_move', { id, stageId, position }),
  sessionDelete: (id: string) => invoke<void>('session_delete', { id }),
  completeAndFollow: (id: string, studentQuestions: string, reflection: string, next?: { title: string; stageId: string }) =>
    invoke<UnitDetail>('session_complete_and_follow', { id, studentQuestions, reflection, next: next ?? null }),
  sessionsForRecord: (classId: string, date: string) => invoke<SessionOption[]>('sessions_for_record', { classId, date }),
};
