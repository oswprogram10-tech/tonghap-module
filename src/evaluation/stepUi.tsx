// 단계 화면의 공통 조각(설계 25): 단계 표시, 구분 단추, 근거 고르기.
import type { ReactNode } from 'react';
import type { Bundle, Evidence } from './api';
import { EvidenceDetail, KIND_LABEL, evidenceDate, evidenceLabel, evidenceSummary, isNegative } from './evidenceUi';
import type { Step } from './stepLogic';

export interface StepInfo {
  title: string;
  summary: string;
  disabled?: boolean;
}

/** 4칸 단계 표시. 지난 단계는 ✓, 지금 단계는 채운 칸. 지난 단계는 눌러서 돌아갈 수 있다. */
export function StepHeader({ steps, current, doneThrough, onGo }: { steps: StepInfo[]; current: Step; doneThrough: Step | 0; onGo: (s: Step) => void }) {
  return (
    <nav aria-label="진행 단계">
      <ol className="es-steps">
        {steps.map((s, i) => {
          const n = (i + 1) as Step;
          const state = n === current ? 'current' : n <= doneThrough ? 'done' : 'todo';
          return (
            <li key={n} className={`es-step es-step-${state}`}>
              <button type="button" className="es-step-btn" aria-current={n === current ? 'step' : undefined} disabled={s.disabled} onClick={() => onGo(n)}>
                <span className="es-step-mark" aria-hidden="true">
                  {state === 'done' ? '✓' : n}
                </span>
                <span className="es-step-text">
                  <b>{s.title}</b>
                  <span className="es-step-sum">{s.summary}</span>
                  {state === 'done' && <span className="es-sr"> (끝낸 단계)</span>}
                  {state === 'current' && <span className="es-sr"> (지금 단계)</span>}
                </span>
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

/** 옅은 바탕 트랙 안의 흰 선택칸(시안). */
export function Seg<T extends string | number>({
  label,
  value,
  options,
  onPick,
  disabled,
}: {
  label: string;
  value: T;
  options: { value: T; label: ReactNode; title?: string; disabled?: boolean }[];
  onPick: (v: T) => void;
  disabled?: boolean;
}) {
  return (
    <div className="es-seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={String(o.value)} type="button" className={value === o.value ? 'es-seg-btn on' : 'es-seg-btn'} aria-pressed={value === o.value} disabled={disabled || o.disabled} title={o.title} onClick={() => onPick(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

interface EvidenceStepProps {
  bundle: Bundle;
  excluded: string[];
  open: string | null;
  onOpen: (id: string | null) => void;
  onToggle: (id: string) => void;
  includedCount: number;
  enough: boolean;
  emptyText: string;
  shortText: string;
  /** 행동특성이면 「지도가 필요한 모습」 표시와 이어진 변화 기록을 보여 준다. */
  guidanceMarks: boolean;
  onNext: () => void;
}

/** 1단계: 근거 고르기. 근거 한 줄을 누르면 그 자리에서 펼쳐진다. */
export function EvidenceStep({ bundle, excluded, open, onOpen, onToggle, includedCount, enough, emptyText, shortText, guidanceMarks, onNext }: EvidenceStepProps) {
  const byId = new Map(bundle.evidence.map((e) => [e.id, e]));
  return (
    <div className="es-panel">
      <div className="es-panel-head">
        <h3>근거 고르기</h3>
        <span className="es-muted">
          {includedCount}/{bundle.evidence.length}개를 초안에 씁니다
        </span>
      </div>
      <p className="diag-hint">
        체크하면 초안을 만들 때 씁니다. 한 줄을 누르면 원래 기록이 펼쳐집니다. 이름은 「학생」, 함께 기록된 친구 이름은 「친구」로 바뀌어 있습니다.
        {guidanceMarks && ' 「지도가 필요한 모습」은 단점 근거이며 변화 가능성과 함께 쓰입니다.'}
      </p>
      {!enough && (
        <p className="diag-error" role="alert">
          {shortText}
        </p>
      )}
      {bundle.evidence.length === 0 && <p className="diag-hint">{emptyText}</p>}
      <div className="es-evlist" role="group" aria-label="근거 고르기">
        {bundle.evidence.map((e) => {
          const on = !excluded.includes(e.id);
          const isOpen = open === e.id;
          const pairOf = e.kind === 'observation' ? e.pair : undefined;
          return (
            <div key={e.id} className={on ? 'es-ev on' : 'es-ev'}>
              <label className="es-ev-check">
                <input type="checkbox" checked={on} onChange={() => onToggle(e.id)} aria-label={`${evidenceLabel(e)} 근거로 쓰기`} />
                <span className="es-sr">{on ? '초안에 씀' : '초안에 쓰지 않음'}</span>
              </label>
              <button type="button" className="es-ev-row" aria-expanded={isOpen} onClick={() => onOpen(isOpen ? null : e.id)}>
                <span className="es-ev-date">{evidenceDate(e)}</span>
                <span className="rec-tag">{KIND_LABEL[e.kind]}</span>
                {guidanceMarks && isNegative(e) && <span className="rec-tag rec-tag-guidance">지도가 필요한 모습</span>}
                <span className="es-ev-text">{evidenceSummary(e)}</span>
                {pairOf && byId.has(pairOf) && <span className="bh-pair">↳ 앞선 「지도가 필요한 모습」 뒤에 이어진 변화 기록</span>}
                <span className="es-ev-chev" aria-hidden="true">
                  {isOpen ? '▴' : '▾'}
                </span>
                <span className="es-sr">{isOpen ? '접기' : '펼쳐 보기'}</span>
              </button>
              {isOpen && <EvidenceDetail e={e} bundle={bundle.standardsText} />}
            </div>
          );
        })}
      </div>
      <div className="es-nav">
        <span className="es-grow" />
        <button type="button" className="es-btn es-btn-primary" onClick={onNext}>
          다음: 초안 만들기
        </button>
      </div>
    </div>
  );
}

/** 문장의 근거 카드(3단계 오른쪽). */
export function EvidenceCard({ e, bundle, onRemove }: { e: Evidence | undefined; bundle: Bundle; onRemove: () => void }) {
  if (!e) {
    return (
      <article className="es-card es-card-bad">
        <div className="es-card-head">없는 기록</div>
        <p>이 근거 기록을 찾을 수 없습니다. 근거에서 빼 주세요.</p>
        <button type="button" className="es-btn es-btn-quiet" aria-label="근거 없는 기록 빼기" onClick={onRemove}>
          이 근거 빼기
        </button>
      </article>
    );
  }
  const neg = isNegative(e);
  return (
    <article className={neg ? 'es-card es-card-warn' : 'es-card'}>
      <div className="es-card-head">
        {evidenceDate(e)} · {KIND_LABEL[e.kind]}
        {e.kind !== 'habit' && e.topic ? ` · ${e.topic}` : ''}
        {neg && <span className="rec-tag rec-tag-guidance">지도가 필요한 모습</span>}
      </div>
      <EvidenceBody e={e} />
      {e.kind !== 'habit' && e.standards.length > 0 && (
        <p className="es-card-tags">
          {e.standards.map((c) => (
            <code key={c} title={bundle.standardsText[c] ?? ''}>
              {c}
            </code>
          ))}
        </p>
      )}
      <button type="button" className="es-btn es-btn-quiet" aria-label={`근거 ${evidenceLabel(e)} 빼기`} onClick={onRemove}>
        이 근거 빼기
      </button>
    </article>
  );
}

function EvidenceBody({ e }: { e: Evidence }) {
  if (e.kind === 'observation') {
    return (
      <>
        <p className="es-card-text">{e.text || e.quote || '(메모 없음)'}</p>
        {e.text && e.quote && <p className="es-card-sub">음성 메모 자막: “{e.quote}”</p>}
        {e.tags.length > 0 && <p className="es-card-sub">태그: {e.tags.join(', ')}</p>}
        {e.together && <p className="es-card-sub">친구와 함께 기록된 관찰</p>}
      </>
    );
  }
  if (e.kind === 'habit') {
    return (
      <p className="es-card-text">
        「{e.title}」 {e.schoolDays}일 중 {e.doneDays}일 실천
      </p>
    );
  }
  return <p className="es-card-text">{e.title}</p>;
}
