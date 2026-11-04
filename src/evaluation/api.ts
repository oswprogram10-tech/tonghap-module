// src-tauri/src/db/evaluation 의 명령과 1:1로 대응한다.
import { invoke } from '@tauri-apps/api/core';

export type SubjectCode = '바' | '슬' | '즐';
export const SUBJECT_LIST: { code: SubjectCode; name: string }[] = [
  { code: '바', name: '바른 생활' },
  { code: '슬', name: '슬기로운 생활' },
  { code: '즐', name: '즐거운 생활' },
];

export type EvalStyle = 'ham' | 'eum' | 'boim';
export const STYLE_LABEL: Record<EvalStyle, string> = { ham: '~함', eum: '~음 (하였음)', boim: '~하는 모습을 보임' };

export const LEN_MIN = 150;
export const LEN_MAX = 400;
export const MIN_EVIDENCE = 3;

export interface Sentence {
  text: string;
  evidence: string[];
}

export type Evidence =
  | {
      kind: 'observation';
      id: string;
      date: string;
      topic: string | null;
      standards: string[];
      tags: string[];
      text: string;
      quote?: string;
      together: boolean;
    }
  | { kind: 'habit'; id: string; title: string; doneDays: number; schoolDays: number }
  | { kind: 'session'; id: string; date: string; topic: string | null; title: string; standards: string[] };

export interface Bundle {
  student: { alias: string };
  subject: SubjectCode;
  period: { from: string; to: string };
  evidence: Evidence[];
  standardsText: Record<string, string>;
}

export interface Warning {
  code: 'noEvidence' | 'forbidden' | 'otherName' | 'mismatch' | 'length';
  message: string;
}

export interface DraftCheck {
  sentences: Warning[][];
  totalChars: number;
  length: Warning | null;
  canConfirm: boolean;
}

export interface StoredDraft {
  id: string;
  sentences: Sentence[];
  status: 'draft' | 'confirmed';
  generator: string;
  confirmedAt: string | null;
  updatedAt: string;
}

export interface DraftView {
  student: { id: string; number: number; name: string };
  subject: SubjectCode;
  from: string;
  to: string;
  bundle: Bundle;
  enough: boolean;
  minEvidence: number;
  draft: StoredDraft | null;
  check: DraftCheck | null;
}

export interface StudentRow {
  studentId: string;
  number: number;
  name: string;
  evidenceCount: number;
  status: 'none' | 'draft' | 'confirmed';
}

export interface EvalSettings {
  style: EvalStyle;
  targetChars: number;
  extraForbidden: string[];
}

export interface LlmStatus {
  modelId: string;
  modelLabel: string;
  modelReady: boolean;
  engineReady: boolean;
  running: boolean;
}

export interface LlmDraftResult {
  view: DraftView;
  /** `llm:<모델 id>` 또는 `template` */
  generator: string;
  fellBack: boolean;
  notice: string | null;
  detail: string | null;
}

/** 초안을 만드는 방식. */
export type GeneratorChoice = 'template' | 'llm';

/** 저장된 초안의 `generator` 값을 화면 문구로. */
export function generatorLabel(g: string): string {
  if (g.startsWith('llm:')) return '언어 모델';
  if (g === 'manual') return '직접 고침';
  return '템플릿';
}

export const CANCELLED_TEXT = '초안 만들기를 취소했습니다';

export interface DraftKey {
  studentId: string;
  subject: SubjectCode;
  from: string;
  to: string;
}

export const evalApi = {
  settingsGet: () => invoke<EvalSettings>('eval_settings_get'),
  settingsSet: (input: EvalSettings) => invoke<EvalSettings>('eval_settings_set', { input }),
  status: (classId: string, subject: SubjectCode, from: string, to: string) => invoke<StudentRow[]>('eval_status', { classId, subject, from, to }),
  view: (key: DraftKey) => invoke<DraftView>('eval_view', { key }),
  generate: (key: DraftKey, excludedIds: string[]) => invoke<DraftView>('eval_generate', { input: { ...key, excludedIds } }),
  llmStatus: () => invoke<LlmStatus>('eval_llm_status'),
  generateLlm: (key: DraftKey, excludedIds: string[]) => invoke<LlmDraftResult>('eval_generate_llm', { input: { ...key, excludedIds } }),
  llmCancel: () => invoke<void>('eval_llm_cancel'),
  save: (key: DraftKey, sentences: Sentence[]) => invoke<DraftView>('eval_save', { input: { ...key, sentences } }),
  check: (key: DraftKey, sentences: Sentence[]) => invoke<DraftCheck>('eval_check', { input: { ...key, sentences } }),
  confirm: (key: DraftKey) => invoke<DraftView>('eval_confirm', { key }),
  exportTxt: (classId: string, from: string, to: string, path: string) => invoke<number>('eval_export', { classId, from, to, path }),
};

export const joinSentences = (sentences: Sentence[]) => sentences.map((s) => s.text.trim()).filter(Boolean).join(' ');

/** 학기 기본 기간: 3월~7월 / 8월~이듬해 2월. */
export function semesterPeriod(schoolYear: number, semester: 1 | 2): { from: string; to: string } {
  return semester === 1 ? { from: `${schoolYear}-03-02`, to: `${schoolYear}-07-31` } : { from: `${schoolYear}-08-01`, to: `${schoolYear + 1}-02-28` };
}

export function currentSemester(today = new Date()): 1 | 2 {
  const m = today.getMonth() + 1;
  return m >= 3 && m <= 7 ? 1 : 2;
}
