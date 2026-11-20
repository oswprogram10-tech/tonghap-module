// 교과 평어와 행동특성 및 종합의견을 같은 단계 화면(DraftFlow)으로 다루기 위한 어댑터(설계 25).
// Rust 명령은 그대로 부르고, 응답 모양만 화면용으로 맞춘다.
import {
  behaviorApi,
  evalApi,
  type Bundle,
  type BehaviorCheck,
  type BehaviorView,
  type DraftCheck,
  type DraftKey,
  type DraftView,
  type OpinionKey,
  type Sentence,
} from './api';
import type { DraftStatus } from './stepLogic';

export interface FlowWarning {
  code: string;
  message: string;
}

export interface FlowCheck {
  sentences: FlowWarning[][];
  overall: FlowWarning[];
  chars: number;
  length: FlowWarning | null;
  canConfirm: boolean;
}

export interface FlowView {
  student: { id: string; number: number; name: string };
  bundle: Bundle;
  sentences: Sentence[];
  status: DraftStatus;
  generator: string | null;
  unchangedFromModel: boolean;
  check: FlowCheck | null;
  targetChars: number;
  /** 행동특성: 저장된 「학기 나누기」 값 */
  split: boolean | null;
}

export interface FlowLlmResult {
  view: FlowView;
  fellBack: boolean;
  notice: string | null;
}

export interface DraftAdapter {
  kind: 'subject' | 'behavior';
  /** 화면에서 부르는 이름: 「이 교과 평어」 */
  noun: string;
  maxSentences: number;
  copyDone: string;
  emptyEvidence: string;
  shortOfEvidence: (n: number) => string;
  load: () => Promise<FlowView>;
  generate: (excluded: string[]) => Promise<FlowView>;
  generateLlm: (excluded: string[]) => Promise<FlowLlmResult>;
  save: (sentences: Sentence[]) => Promise<FlowView>;
  check: (sentences: Sentence[]) => Promise<FlowCheck>;
  confirm: () => Promise<FlowView>;
}

const fromDraftCheck = (c: DraftCheck): FlowCheck => ({ sentences: c.sentences, overall: [], chars: c.totalChars, length: c.length, canConfirm: c.canConfirm });

const fromBehaviorCheck = (c: BehaviorCheck): FlowCheck => ({ sentences: c.sentences, overall: c.overall, chars: c.totalChars, length: c.length, canConfirm: c.canConfirm });

const fromDraft = (v: DraftView, target: number): FlowView => ({
  student: v.student,
  bundle: v.bundle,
  sentences: v.draft?.sentences ?? [],
  status: v.draft?.status ?? 'none',
  generator: v.draft?.generator ?? null,
  unchangedFromModel: v.draft?.unchangedFromModel ?? false,
  check: v.check ? fromDraftCheck(v.check) : null,
  targetChars: target,
  split: null,
});

const fromBehavior = (v: BehaviorView): FlowView => ({
  student: v.student,
  bundle: v.bundle,
  sentences: v.opinion?.sentences ?? [],
  status: v.opinion?.status ?? 'none',
  generator: v.opinion?.generator ?? null,
  unchangedFromModel: v.opinion?.unchangedFromModel ?? false,
  check: v.check ? fromBehaviorCheck(v.check) : null,
  targetChars: v.targetChars,
  split: v.opinion ? v.opinion.splitSemesters : null,
});

export function subjectAdapter(key: DraftKey, target: number): DraftAdapter {
  return {
    kind: 'subject',
    noun: '이 교과 평어',
    maxSentences: 12,
    copyDone: '복사했습니다. 나이스에 붙여 넣으세요.',
    emptyEvidence: '이 기간에 이 교과의 기록이 없습니다.',
    shortOfEvidence: (n) => `기록이 부족합니다(${n}개). 이 교과 관찰 기록을 더 남겨 주세요 (초안은 근거 3개부터 만들 수 있어요).`,
    load: async () => fromDraft(await evalApi.view(key), target),
    generate: async (excluded) => fromDraft(await evalApi.generate(key, excluded), target),
    generateLlm: async (excluded) => {
      const r = await evalApi.generateLlm(key, excluded);
      return { view: fromDraft(r.view, target), fellBack: r.fellBack, notice: r.notice };
    },
    save: async (sentences) => fromDraft(await evalApi.save(key, sentences), target),
    check: async (sentences) => fromDraftCheck(await evalApi.check(key, sentences)),
    confirm: async () => fromDraft(await evalApi.confirm(key), target),
  };
}

/** `getSplit`은 부를 때마다 「학기 나누기」 최신 값을 돌려준다(어댑터를 새로 만들지 않아도 되게). */
export function behaviorAdapter(key: OpinionKey, getSplit: () => boolean): DraftAdapter {
  return {
    kind: 'behavior',
    noun: '행동특성 및 종합의견',
    maxSentences: 24,
    copyDone: '복사했습니다. 나이스 「행동특성 및 종합의견」에 붙여 넣으세요.',
    emptyEvidence: '이 기간에 이 아이의 관찰 기록이 없습니다.',
    shortOfEvidence: (n) => `기록이 부족합니다(${n}개). 이 기간의 관찰 기록을 더 남겨 주세요 (초안은 근거 3개부터 만들 수 있어요).`,
    load: async () => fromBehavior(await behaviorApi.view(key)),
    generate: async (excluded) => fromBehavior(await behaviorApi.generate(key, excluded, getSplit())),
    generateLlm: async (excluded) => {
      const r = await behaviorApi.generateLlm(key, excluded, getSplit());
      return { view: fromBehavior(r.view), fellBack: r.fellBack, notice: r.notice };
    },
    save: async (sentences) => fromBehavior(await behaviorApi.save(key, sentences, getSplit())),
    check: async (sentences) => fromBehaviorCheck(await behaviorApi.check(key, sentences, getSplit())),
    confirm: async () => fromBehavior(await behaviorApi.confirm(key)),
  };
}
