// 통합본(세 교과를 합친 NEIS 「성취수준 및 특기사항」)의 학생 단계 화면(설계 25).
// ① 세 교과 확인 → ② 합치기 · 다듬기 → ③ 전체 확인(체크 하나) → ④ 확정 · 복사.
import { useCallback, useEffect, useMemo, useRef, useState, type MutableRefObject } from 'react';
import { copyText } from '../lib/clipboard';
import { errorText } from '../records/api';
import { REVIEW_NOTICE, UNCHANGED_NOTICE, evalApi, type CombinedCheck, type CombinedKey, type CombinedView, type SubjectCode } from './api';
import { confirmLabel, startStep, type Step } from './stepLogic';
import { StepHeader, type StepInfo } from './stepUi';

interface Props {
  studentId: string;
  from: string;
  to: string;
  onChanged: () => void;
  onNext: (() => void) | null;
  /** 아직 확정하지 않은 교과의 단계 화면으로 간다. */
  onGoSubject: (code: SubjectCode) => void;
  saveRef: MutableRefObject<(() => Promise<boolean>) | null>;
}

const STATUS_TEXT = { none: '없음', draft: '초안', confirmed: '확정' } as const;

export default function CombinedFlow({ studentId, from, to, onChanged, onNext, onGoSubject, saveRef }: Props) {
  const key: CombinedKey = useMemo(() => ({ studentId, from, to }), [studentId, from, to]);
  const [view, setView] = useState<CombinedView | null>(null);
  const [text, setText] = useState('');
  const [reviewed, setReviewed] = useState(false);
  const [check, setCheck] = useState<CombinedCheck | null>(null);
  const [step, setStep] = useState<Step>(1);
  const [busy, setBusy] = useState(false);
  const [askOverwrite, setAskOverwrite] = useState(false);
  const [askUnchanged, setAskUnchanged] = useState(false);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const first = useRef(true);

  const apply = useCallback((v: CombinedView) => {
    setView(v);
    setText(v.combined?.text ?? '');
    setReviewed(v.combined?.reviewed ?? false);
    setCheck(v.check);
  }, []);

  useEffect(() => {
    let live = true;
    evalApi.combinedView(key).then(
      (v) => {
        if (!live) return;
        apply(v);
        if (first.current) {
          first.current = false;
          setStep(startStep(v.combined?.status ?? 'none', v.combined ? [{ reviewed: v.combined.reviewed }] : []).step);
        }
      },
      (e) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [key, apply]);

  const saved = view?.combined?.text ?? '';
  const dirty = view?.combined != null && (text !== saved || reviewed !== view.combined.reviewed);

  // 편집하는 동안 경고를 다시 계산한다(저장하지 않고 검사만).
  useEffect(() => {
    if (!view?.combined) return;
    const n = ++seq.current;
    const t = setTimeout(() => {
      evalApi.combinedCheck(key, text).then(
        (c) => n === seq.current && setCheck(c),
        (e) => n === seq.current && setError(errorText(e)),
      );
    }, 250);
    return () => clearTimeout(t);
  }, [text, key, view]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setInfo(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    saveRef.current = async () => {
      if (!dirty || !view?.combined) return true;
      try {
        apply(await evalApi.combinedSave(view.combined.id, text, reviewed));
        onChanged();
        return true;
      } catch (e) {
        setError(errorText(e));
        return false;
      }
    };
    return () => {
      saveRef.current = null;
    };
  }, [dirty, view, text, reviewed, apply, onChanged, saveRef]);

  const combine = (overwrite: boolean) =>
    run(async () => {
      setAskOverwrite(false);
      apply(await evalApi.combine(key, overwrite));
      setInfo('세 교과를 합쳤습니다. 읽어 보고 자연스럽게 다듬어 주세요.');
      onChanged();
    });

  const saveNow = () =>
    run(async () => {
      if (!view?.combined) return;
      apply(await evalApi.combinedSave(view.combined.id, text, reviewed));
      setInfo('저장했습니다.');
      onChanged();
    });

  /** 확정. 언어 모델이 쓴 평어를 한 글자도 고치지 않았으면 먼저 한 번 더 묻는다(막지는 않는다). */
  const confirm = (skipAsk = false) =>
    run(async () => {
      if (!view?.combined) return;
      setAskUnchanged(false);
      let v = view;
      if (dirty) {
        v = await evalApi.combinedSave(view.combined.id, text, reviewed);
        apply(v);
      }
      if (!skipAsk && v.combined?.unchangedFromModel) {
        setAskUnchanged(true);
        return;
      }
      apply(await evalApi.combinedConfirm(view.combined.id));
      setInfo('확정했습니다. 이제 복사할 수 있어요.');
      onChanged();
    });

  const unconfirm = () =>
    run(async () => {
      if (!view?.combined) return;
      apply(await evalApi.combinedUnconfirm(view.combined.id));
      setInfo('확정을 풀었습니다. 고친 뒤 다시 확정해 주세요.');
      onChanged();
    });

  const copy = () =>
    run(async () => {
      await copyText(saved);
      setInfo('복사했습니다. 나이스 「성취수준 및 특기사항」에 붙여 넣으세요.');
    });

  if (error && !view) return <p className="diag-error">{error}</p>;
  if (!view) return <p className="diag-note">불러오는 중…</p>;

  const c = view.combined;
  const status = c?.status ?? 'none';
  const confirmedNow = status === 'confirmed' && !dirty;
  const confirmedSubjects = view.subjects.filter((s) => s.confirmed).length;
  const label = confirmLabel({ count: text.trim() ? 1 : 0, firstUnreviewed: reviewed ? null : 0, canConfirm: !!check?.canConfirm, unit: '통합본' });
  const blocked = check ? check.warnings.filter((w) => w.code === 'forbidden' || w.code === 'otherName') : [];

  const steps: StepInfo[] = [
    { title: '세 교과 확인', summary: `확정 ${confirmedSubjects}/3` },
    { title: '합치기 · 다듬기', summary: c ? '합침' : '아직 없음' },
    { title: '전체 확인', summary: c ? (reviewed ? '확인함' : '확인 전') : '합친 뒤', disabled: !c },
    { title: '확정 · 복사', summary: confirmedNow ? '확정함' : 'NEIS에 붙여 넣기', disabled: !c },
  ];
  const doneThrough: Step | 0 = confirmedNow ? 4 : ((step - 1) as Step | 0);
  const go = (s: Step) => {
    setAskOverwrite(false);
    setStep(s);
  };

  const stats = check ? (
    <p className="ev-length" role="status">
      {check.chars}자 · NEIS {check.bytes}바이트{check.lengthNotice ? ` — ${check.lengthNotice}` : ''}
    </p>
  ) : null;
  const warnings =
    check && check.warnings.length > 0 ? (
      <ul className="ev-warns" aria-label="통합본 경고">
        {check.warnings.map((w, i) => (
          <li key={i} className={`ev-warn ev-warn-${w.code}`}>
            ⚠ {w.message}
          </li>
        ))}
      </ul>
    ) : null;

  return (
    <section className="ev-combined es-flow" aria-label={`${view.student.number}번 ${view.student.name} 통합본`}>
      <StepHeader steps={steps} current={step} doneThrough={doneThrough} onGo={go} />
      <div className="es-statusline">
        <span className={`ev-status ev-status-${status}`}>{status === 'none' ? '통합본 없음' : `통합본 ${STATUS_TEXT[status]}`}</span>
        <span className="es-muted">1~2학년 바른 생활·슬기로운 생활·즐거운 생활은 나이스 「성취수준 및 특기사항」에 통합해서 입력합니다.</span>
      </div>
      {error && <p className="diag-error">{error}</p>}
      {info && (
        <p className="rec-toast ev-toast" role="status">
          {info}
        </p>
      )}
      {c?.stale && (
        <p className="ev-fallback" role="alert">
          교과 평어가 바뀌었습니다. 필요하면 「세 교과 다시 합치기」를 눌러 주세요(지금 통합본은 자동으로 바뀌지 않습니다).
        </p>
      )}

      {step === 1 && (
        <div className="es-panel">
          <div className="es-panel-head">
            <h3>세 교과 확인</h3>
            <span className="es-muted">세 교과 평어를 모두 확정해야 합칠 수 있어요.</span>
          </div>
          <ul className="ev-subjects" aria-label="교과별 확정 상태">
            {view.subjects.map((s) => (
              <li key={s.code} className={s.confirmed ? 'ev-subject ev-subject-ok' : 'ev-subject'}>
                <span>
                  {s.confirmed ? '✓' : '○'} {s.name} {s.confirmed ? '확정' : '미확정'}
                </span>
                {!s.confirmed && (
                  <button type="button" className="es-btn" onClick={() => onGoSubject(s.code)}>
                    {s.name} 확정하러 가기
                  </button>
                )}
              </li>
            ))}
          </ul>
          <div className="es-nav">
            <span className="es-grow" />
            <button type="button" className="es-btn es-btn-primary" onClick={() => go(2)}>
              다음: 합치기
            </button>
          </div>
        </div>
      )}

      {step === 2 && (
        <div className="es-panel">
          <div className="es-panel-head">
            <h3>합치기 · 다듬기</h3>
          </div>
          <div className="es-row es-wrap">
            <button type="button" className="es-btn es-btn-primary" disabled={busy || !view.canCombine} onClick={() => (c ? setAskOverwrite(true) : combine(false))}>
              {c ? '세 교과 다시 합치기' : '세 교과 합치기'}
            </button>
            {!view.canCombine && <span className="diag-hint">세 교과 평어를 모두 확정하면 합칠 수 있어요(1단계에서 교과로 가서 확정해 주세요).</span>}
          </div>
          {askOverwrite && (
            <div className="es-ask" role="alert">
              <p>지금 통합본이 교과 평어를 새로 합친 글로 바뀌고, 고친 내용은 사라집니다. 계속할까요?</p>
              <div className="es-row">
                <button type="button" className="es-btn es-btn-primary" disabled={busy} onClick={() => combine(true)}>
                  다시 합치기
                </button>
                <button type="button" className="es-btn" onClick={() => setAskOverwrite(false)}>
                  취소
                </button>
              </div>
            </div>
          )}
          {c && (
            <>
              <textarea className="es-textarea es-combined-text" rows={8} value={text} aria-label="통합본 글" onChange={(e) => setText(e.target.value)} />
              {stats}
              {warnings}
              {blocked.length > 0 && <p className="diag-hint">금지 표현이나 다른 학생 이름을 고쳐야 확정할 수 있어요.</p>}
            </>
          )}
          <div className="es-nav">
            <button type="button" className="es-btn es-btn-quiet" onClick={() => go(1)}>
              ← 세 교과 확인
            </button>
            <span className="es-grow" />
            {c && (
              <button type="button" className="es-btn es-btn-primary" onClick={() => go(3)}>
                다음: 전체 확인
              </button>
            )}
          </div>
        </div>
      )}

      {step === 3 && c && (
        <div className="es-panel">
          <div className="es-panel-head">
            <h3>전체 확인</h3>
            <span className={reviewed ? 'es-chip es-chip-ok' : 'es-chip es-chip-todo'}>{reviewed ? '✓ 확인함' : '아직 확인 안 함'}</span>
          </div>
          <p className="es-question">세 교과 문장의 근거와 아이의 실제 모습에 맞게 글이 다듬어졌나요?</p>
          <p className="es-preview-text es-combined-read">{text}</p>
          {stats}
          {warnings}
          <div className="es-actions">
            <button
              type="button"
              className="es-btn es-btn-primary es-btn-big"
              onClick={() => {
                setReviewed(true);
                go(4);
              }}
            >
              맞아요, 확인했어요
            </button>
            <button type="button" className="es-btn" onClick={() => go(2)}>
              돌아가서 고치기
            </button>
            {reviewed && (
              <button type="button" className="es-btn es-btn-quiet" onClick={() => setReviewed(false)}>
                확인 취소
              </button>
            )}
          </div>
        </div>
      )}

      {step === 4 && c && (
        <div className="es-panel es-confirm">
          <div className="es-panel-head">
            <h3>확정 · 복사</h3>
          </div>
          <textarea className="es-textarea es-combined-text" rows={8} value={text} aria-label="통합본 글" onChange={(e) => setText(e.target.value)} />
          {stats}
          {warnings}
          {blocked.length > 0 && <p className="diag-hint">금지 표현이나 다른 학생 이름을 고쳐야 확정할 수 있어요.</p>}
          <div className="es-actions">
            <button type="button" className="es-btn" disabled={busy || !dirty} onClick={saveNow}>
              저장
            </button>
            {confirmedNow ? (
              <button type="button" className="es-btn" disabled={busy} onClick={unconfirm}>
                확정 풀기
              </button>
            ) : (
              <button
                type="button"
                className="es-btn es-btn-primary es-btn-big"
                disabled={busy || label.action === 'none' || label.action === 'blocked'}
                onClick={() => (label.action === 'goto' ? go(3) : confirm())}
              >
                {label.label}
              </button>
            )}
            <button type="button" className="es-btn es-btn-primary" disabled={busy || status !== 'confirmed' || dirty} onClick={copy} title={status === 'confirmed' ? '' : '확정한 뒤에 복사할 수 있어요'}>
              복사
            </button>
            {confirmedNow && onNext && (
              <button type="button" className="es-btn es-btn-quiet" onClick={onNext}>
                다음 학생 →
              </button>
            )}
          </div>
          {status !== 'confirmed' && (
            <p className="diag-hint ev-review-notice" role="note">
              {REVIEW_NOTICE} 확정한 통합본만 복사·내보내기를 할 수 있습니다.
            </p>
          )}
          {askUnchanged && (
            <div className="ev-ask-box" role="alertdialog" aria-label="그대로 확정할까요">
              <p>{UNCHANGED_NOTICE}</p>
              <div className="es-row">
                <button type="button" className="es-btn es-btn-primary" disabled={busy} onClick={() => confirm(true)}>
                  그대로 확정
                </button>
                <button type="button" className="es-btn" onClick={() => setAskUnchanged(false)}>
                  돌아가서 고치기
                </button>
              </div>
            </div>
          )}
          <div className="es-nav">
            <button type="button" className="es-btn es-btn-quiet" onClick={() => go(3)}>
              ← 전체 확인으로
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
