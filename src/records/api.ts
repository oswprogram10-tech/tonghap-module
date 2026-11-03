// src-tauri/src/db 의 명령과 1:1로 대응한다. SQL은 Rust에만 있다.
import { invoke } from '@tauri-apps/api/core';

export interface ClassInfo {
  id: string;
  schoolYear: number;
  grade: number;
  name: string;
  createdAt: string;
}

export interface Student {
  id: string;
  classId: string;
  number: number;
  name: string;
  active: boolean;
  createdAt: string;
}

export interface ImportResult {
  added: number;
  updated: number;
  skipped: string[];
}

export interface Attachment {
  id: string;
  kind: 'photo' | 'audio';
  relPath: string;
  absPath: string;
  mime: string;
  bytes: number;
  transcript: string | null;
  transcriptStatus: 'none' | 'queued' | 'running' | 'done' | 'failed';
  transcriptError: string | null;
  transcriptEdited: boolean;
  transcriptModel: string | null;
  createdAt: string;
}

export interface ObservationInput {
  classId: string;
  observedAt: string;
  topicBookId?: string | null;
  note: string;
  studentIds: string[];
  standardCodes: string[];
  tags: string[];
  /** 연결한 단원 차시(선택). */
  unitSessionId?: string | null;
}

export interface Observation extends ObservationInput {
  id: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  unitSessionTitle: string | null;
  students: { id: string; number: number; name: string }[];
  attachments: Attachment[];
}

export interface ObservationQuery {
  classId: string;
  studentId?: string;
  topicBookId?: string;
  standardCode?: string;
  includeDeleted?: boolean;
  /** 휴지통 보기: 삭제된 기록만. */
  deletedOnly?: boolean;
  limit?: number;
  before?: string;
}

export interface StudentPortfolio {
  student: Student;
  observationCount: number;
  byTopicBook: { topicBookId: string; count: number }[];
  byStandard: { standardCode: string; count: number }[];
  byTag: { tag: string; count: number }[];
  firstObservedAt: string | null;
  lastObservedAt: string | null;
}

export interface ClassOverview {
  students: {
    student: Student;
    total: number;
    byTopicBook: Record<string, number>;
    lastObservedAt: string | null;
  }[];
}

/** 활동 태그(고정 목록). */
export const ACTIVITY_TAGS = ['실천', '탐구', '놀이', '표현', '협력', '배려', '질문', '안전', '습관'] as const;

export const errorText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e));

export const recordsApi = {
  classList: () => invoke<ClassInfo[]>('class_list'),
  classCreate: (schoolYear: number, grade: number, name: string) => invoke<ClassInfo>('class_create', { schoolYear, grade, name }),
  classUpdate: (id: string, schoolYear: number, grade: number, name: string) =>
    invoke<ClassInfo>('class_update', { id, schoolYear, grade, name }),
  classArchive: (id: string) => invoke<void>('class_archive', { id }),
  studentList: (classId: string, includeInactive = false) => invoke<Student[]>('student_list', { classId, includeInactive }),
  studentImport: (classId: string, text: string) => invoke<ImportResult>('student_import', { classId, text }),
  studentUpdate: (id: string, number: number, name: string, active: boolean) =>
    invoke<Student>('student_update', { id, number, name, active }),
  observationCreate: (input: ObservationInput) => invoke<Observation>('observation_create', { input }),
  observationUpdate: (id: string, input: ObservationInput) => invoke<Observation>('observation_update', { id, input }),
  observationDelete: (id: string) => invoke<void>('observation_delete', { id }),
  observationRestore: (id: string) => invoke<void>('observation_restore', { id }),
  observationPurge: (id: string) => invoke<void>('observation_purge', { id }),
  observationList: (query: ObservationQuery) => invoke<Observation[]>('observation_list', { query }),
  attachmentAddFile: (observationId: string, path: string) => invoke<Attachment>('attachment_add_file', { observationId, path }),
  /** 녹음 같은 큰 바이트는 JSON이 아니라 원시 본문으로 보낸다. 헤더 값은 ASCII여야 한다. */
  attachmentAddBytes: (observationId: string, kind: 'audio', mime: string, bytes: Uint8Array) =>
    invoke<Attachment>('attachment_add_bytes', bytes, {
      headers: { 'x-observation-id': observationId, 'x-kind': kind, 'x-mime': mime },
    }),
  attachmentDelete: (id: string) => invoke<void>('attachment_delete', { id }),
  portfolioStudent: (studentId: string) => invoke<StudentPortfolio>('portfolio_student', { studentId }),
  portfolioClass: (classId: string) => invoke<ClassOverview>('portfolio_class', { classId }),
  settingsGet: <T = unknown>(key: string) => invoke<T | null>('settings_get', { key }),
  settingsSet: (key: string, value: unknown) => invoke<unknown>('settings_set', { key, value }),
};
