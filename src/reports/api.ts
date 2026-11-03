// src-tauri/src/db/reports.rs 의 명령과 1:1로 대응한다.
import { invoke } from '@tauri-apps/api/core';
import type { HabitSummary } from '../board/api';

export const MAX_OBSERVATIONS = 4;
export const MAX_PHOTOS = 4;
export const MAX_QUOTES = 4;
export const MAX_MESSAGE = 400;
export const MAX_NOTE_CHARS = 120;
export const SHEET_WIDTH = 1240;
export const SHEET_HEIGHT = 1754;

export interface ReportObservation {
  id: string;
  observedAt: string;
  /** 다른 학생 이름은 이미 "친구"로 바뀌어 있다. */
  note: string;
  standardCodes: string[];
  tags: string[];
  /** 함께 기록된 다른 학생 수(이름은 받지 않는다). */
  othersCount: number;
}

export interface ReportPhoto {
  id: string;
  absPath: string;
}

export interface ReportAudio {
  id: string;
  transcript: string;
}

export interface Candidate {
  observation: ReportObservation;
  soloForStudent: boolean;
  photos: ReportPhoto[];
  audioWithTranscript: ReportAudio[];
}

export interface ReportDraft {
  id: string | null;
  exportedAt: string | null;
  schoolYear: number;
  grade: number;
  className: string;
  student: { id: string; number: number; name: string };
  topicBookId: string | null;
  from: string;
  to: string;
  lessons: string[];
  candidates: Candidate[];
  observationIds: string[];
  attachmentIds: string[];
  quoteAttachmentIds: string[];
  habits: HabitSummary[];
  message: string;
}

export interface ReportInput {
  studentId: string;
  topicBookId: string | null;
  from: string;
  to: string;
  message: string;
  observationIds: string[];
  attachmentIds: string[];
  quoteAttachmentIds: string[];
}

export interface Period {
  from: string;
  to: string;
}

export const reportApi = {
  draft: (studentId: string, topicBookId: string | null, from: string, to: string) =>
    invoke<ReportDraft>('report_draft', { studentId, topicBookId, from, to }),
  save: (input: ReportInput) => invoke<ReportDraft>('report_save', { input }),
  markExported: (id: string) => invoke<void>('report_mark_exported', { id }),
  defaultPeriod: (classId: string, topicBookId: string | null) => invoke<Period>('report_default_period', { classId, topicBookId }),
  /** PNG 바이트는 JSON이 아니라 원시 본문으로 보낸다. 경로는 한글이 있어 percent-encoding한다. */
  writeFile: (path: string, bytes: Uint8Array) =>
    invoke<void>('report_write_file', bytes, { headers: { 'x-path': encodeURIComponent(path) } }),
};

export function inputOf(d: ReportDraft, sel: { observationIds: string[]; attachmentIds: string[]; quoteAttachmentIds: string[]; message: string }): ReportInput {
  return { studentId: d.student.id, topicBookId: d.topicBookId, from: d.from, to: d.to, ...sel };
}
