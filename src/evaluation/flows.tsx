// 교과 평어·행동특성 단계 화면을 각자의 어댑터에 묶는 얇은 껍데기(설계 25).
import { useMemo, useRef, useState, type MutableRefObject } from 'react';
import { errorText } from '../records/api';
import { BEHAVIOR_TARGET_MAX, BEHAVIOR_TARGET_MIN, behaviorApi, type EvalSettings, type GeneratorChoice, type SubjectCode } from './api';
import { behaviorAdapter, subjectAdapter, type DraftAdapter, type FlowView } from './adapters';
import DraftFlow from './DraftFlow';

interface Common {
  studentId: string;
  from: string;
  to: string;
  generator: GeneratorChoice;
  onGenerator: (g: GeneratorChoice) => void;
  llmReady: boolean;
  locked: boolean;
  onChanged: () => void;
  onNext: (() => void) | null;
  saveRef: MutableRefObject<(() => Promise<boolean>) | null>;
}

export function SubjectFlow(props: Common & { subject: SubjectCode; settings: EvalSettings; onGoCombined: () => void }) {
  const { studentId, subject, from, to, settings, onGoCombined, ...rest } = props;
  const target = settings.targetChars;
  const adapter = useMemo(() => subjectAdapter({ studentId, subject, from, to }, target), [studentId, subject, from, to, target]);
  return <DraftFlow adapter={adapter} guidanceMarks={false} onGoCombined={onGoCombined} {...rest} />;
}

export function BehaviorFlow(props: Common & { split: boolean; onSplit: (v: boolean) => void }) {
  const { studentId, from, to, split, onSplit, ...rest } = props;
  const splitRef = useRef(split);
  splitRef.current = split;
  const adapter: DraftAdapter = useMemo(() => {
    const base = behaviorAdapter({ studentId, from, to }, () => splitRef.current);
    const tap = <V extends { split: boolean | null }>(v: V): V => {
      if (v.split !== null) onSplit(v.split);
      return v;
    };
    return {
      ...base,
      load: async () => tap(await base.load()),
      generate: async (x) => tap(await base.generate(x)),
      generateLlm: async (x) => {
        const r = await base.generateLlm(x);
        tap(r.view);
        return r;
      },
      save: async (x) => tap(await base.save(x)),
      confirm: async () => tap(await base.confirm()),
    };
  }, [studentId, from, to, onSplit]);
  const extra = (view: FlowView) => (
    <div className="es-extra">
      <label className="es-check">
        <input type="checkbox" checked={split} onChange={(e) => onSplit(e.target.checked)} />
        <span>학기 나누기 (1학기·2학기 따로 쓰고 「(1학기)」「(2학기)」를 붙임)</span>
      </label>
      <BehaviorTargetField initial={view.targetChars} />
    </div>
  );
  return <DraftFlow adapter={adapter} guidanceMarks recheckKey={split} generateExtra={extra} {...rest} />;
}

/** 행동특성 및 종합의견의 목표 글자 수. */
export function BehaviorTargetField({ initial }: { initial: number }) {
  const [target, setTarget] = useState(String(initial));
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const save = async () => {
    setMsg(null);
    setErr(null);
    try {
      const n = await behaviorApi.targetSet(Number(target));
      setTarget(String(n));
      setMsg(`목표 글자 수를 ${n}자로 저장했습니다. 다음에 초안을 만들 때부터 적용됩니다.`);
    } catch (e) {
      setErr(errorText(e));
    }
  };
  return (
    <div className="es-row es-wrap">
      <label className="rec-field">
        행동특성 목표 글자 수 ({BEHAVIOR_TARGET_MIN}~{BEHAVIOR_TARGET_MAX})
        <input type="number" min={BEHAVIOR_TARGET_MIN} max={BEHAVIOR_TARGET_MAX} step={50} value={target} onChange={(e) => setTarget(e.target.value)} />
      </label>
      <button type="button" className="es-btn" onClick={save}>
        글자 수 저장
      </button>
      {msg && (
        <span className="rec-toast ev-toast" role="status">
          {msg}
        </span>
      )}
      {err && <span className="diag-error">{err}</span>}
    </div>
  );
}
