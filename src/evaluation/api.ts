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

export const LEN_MIN = 80;
export const LEN_MAX = 400;
export const MIN_EVIDENCE = 3;

export interface Sentence {
  text: string;
  evidence: string[];
  /** 선생님이 「근거 확인」을 눌렀는가(AI·템플릿 문장을 그대로 입력하지 않도록 하는 장치). */
  reviewed: boolean;
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
      /** 「지도가 필요한 모습」 기록(행동특성 및 종합의견) */
      negative?: boolean;
      /** 앞선 「지도가 필요한 모습」 기록의 id(그 뒤에 이어진 긍정 기록) */
      pair?: string;
    }
  | { kind: 'habit'; id: string; title: string; doneDays: number; schoolDays: number }
  | { kind: 'session'; id: string; date: string; topic: string | null; title: string; standards: string[] };

export interface Bundle {
  student: { alias: string };
  subject: SubjectCode | '행';
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
  allReviewed: boolean;
}

export interface StoredDraft {
  id: string;
  sentences: Sentence[];
  status: 'draft' | 'confirmed';
  generator: string;
  confirmedAt: string | null;
  updatedAt: string;
  /** 언어 모델이 만든 글을 한 글자도 고치지 않았다. */
  unchangedFromModel: boolean;
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

export interface CombinedWarning {
  code: 'forbidden' | 'otherName' | 'nounEnding' | 'symbol' | 'missingSubject';
  message: string;
}

export interface CombinedCheck {
  warnings: CombinedWarning[];
  chars: number;
  bytes: number;
  lengthNotice: string | null;
  canConfirm: boolean;
}

export interface CombinedKey {
  studentId: string;
  from: string;
  to: string;
}

export interface CombinedView {
  student: { id: string; number: number; name: string };
  from: string;
  to: string;
  subjects: { code: SubjectCode; name: string; confirmed: boolean }[];
  canCombine: boolean;
  combined: {
    id: string;
    text: string;
    status: 'draft' | 'confirmed';
    confirmedAt: string | null;
    updatedAt: string;
    stale: boolean;
    reviewed: boolean;
    unchangedFromModel: boolean;
  } | null;
  check: CombinedCheck | null;
}

export interface CombinedRow {
  studentId: string;
  status: 'none' | 'draft' | 'confirmed';
  stale: boolean;
}

export type ExportFormat = 'combined' | 'bySubject' | 'behavior';

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
  exportTxt: (classId: string, from: string, to: string, path: string, format: ExportFormat) =>
    invoke<number>('eval_export', { classId, from, to, path, format }),
  combinedView: (key: CombinedKey) => invoke<CombinedView>('eval_combined_view', { key }),
  combinedStatus: (classId: string, from: string, to: string) => invoke<Record<string, CombinedRow>>('eval_combined_status', { classId, from, to }),
  combine: (key: CombinedKey, overwrite: boolean) => invoke<CombinedView>('eval_combine', { input: { ...key, overwrite } }),
  combinedSave: (id: string, text: string, reviewed?: boolean) => invoke<CombinedView>('eval_combined_save', { id, text, reviewed }),
  combinedCheck: (key: CombinedKey, text: string) => invoke<CombinedCheck>('eval_combined_check', { input: { ...key, text } }),
  combinedConfirm: (id: string) => invoke<CombinedView>('eval_combined_confirm', { id }),
  combinedUnconfirm: (id: string) => invoke<CombinedView>('eval_combined_unconfirm', { id }),
};

// ---------- 행동특성 및 종합의견 ----------

export const BEHAVIOR_TARGET_MIN = 150;
export const BEHAVIOR_TARGET_MAX = 800;

export interface BehaviorWarning {
  code: string;
  message: string;
}

export interface BehaviorCheck {
  sentences: BehaviorWarning[][];
  overall: BehaviorWarning[];
  totalChars: number;
  totalBytes: number;
  length: BehaviorWarning | null;
  canConfirm: boolean;
  allReviewed: boolean;
}

export interface StoredOpinion {
  id: string;
  sentences: Sentence[];
  splitSemesters: boolean;
  status: 'draft' | 'confirmed';
  generator: string;
  confirmedAt: string | null;
  updatedAt: string;
  unchangedFromModel: boolean;
}

export interface BehaviorView {
  student: { id: string; number: number; name: string };
  from: string;
  to: string;
  bundle: Bundle;
  enough: boolean;
  minEvidence: number;
  targetChars: number;
  opinion: StoredOpinion | null;
  check: BehaviorCheck | null;
}

export interface BehaviorLlmResult {
  view: BehaviorView;
  generator: string;
  fellBack: boolean;
  notice: string | null;
  detail: string | null;
}

export interface OpinionKey {
  studentId: string;
  from: string;
  to: string;
}

export const behaviorApi = {
  view: (key: OpinionKey) => invoke<BehaviorView>('eval_behavior_view', { key }),
  status: (classId: string, from: string, to: string) => invoke<StudentRow[]>('eval_behavior_status', { classId, from, to }),
  generate: (key: OpinionKey, excludedIds: string[], splitSemesters: boolean) =>
    invoke<BehaviorView>('eval_behavior_generate', { input: { ...key, excludedIds, splitSemesters } }),
  generateLlm: (key: OpinionKey, excludedIds: string[], splitSemesters: boolean) =>
    invoke<BehaviorLlmResult>('eval_behavior_generate_llm', { input: { ...key, excludedIds, splitSemesters } }),
  save: (key: OpinionKey, sentences: Sentence[], splitSemesters: boolean) =>
    invoke<BehaviorView>('eval_behavior_save', { input: { ...key, sentences, splitSemesters } }),
  check: (key: OpinionKey, sentences: Sentence[], splitSemesters: boolean) =>
    invoke<BehaviorCheck>('eval_behavior_check', { input: { ...key, sentences, splitSemesters } }),
  confirm: (key: OpinionKey) => invoke<BehaviorView>('eval_behavior_confirm', { key }),
  targetSet: (targetChars: number) => invoke<number>('eval_behavior_target_set', { targetChars }),
};

/** AI·템플릿 문장 확인 안내(확정 버튼 옆). */
export const REVIEW_NOTICE =
  'AI나 템플릿이 만든 문장을 그대로 입력하지 않도록 2026 기재요령이 정하고 있습니다. 근거와 실제 모습을 확인해 주세요.';
/** 언어 모델이 만든 글을 고치지 않고 확정하려 할 때의 확인 창 문구. */
export const UNCHANGED_NOTICE =
  '언어 모델이 만든 문장을 고치지 않았습니다. 기재요령은 AI가 만든 문장을 그대로 입력하지 않도록 합니다. 그대로 확정할까요?';

export const joinSentences = (sentences: Sentence[]) => sentences.map((s) => s.text.trim()).filter(Boolean).join(' ');

/** 학기 기본 기간: 3월~7월 / 8월~이듬해 2월. */
export function semesterPeriod(schoolYear: number, semester: 1 | 2): { from: string; to: string } {
  return semester === 1 ? { from: `${schoolYear}-03-02`, to: `${schoolYear}-07-31` } : { from: `${schoolYear}-08-01`, to: `${schoolYear + 1}-02-28` };
}

export function currentSemester(today = new Date()): 1 | 2 {
  const m = today.getMonth() + 1;
  return m >= 3 && m <= 7 ? 1 : 2;
}
