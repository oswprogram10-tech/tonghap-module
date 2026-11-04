import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { errorText } from '../records/api';
import { evalApi, type CombinedCheck, type CombinedKey, type CombinedView } from './api';

interface Props {
  studentId: string;
  from: string;
  to: string;
  /** 교과 평어가 바뀔 때마다 늘어난다(확정 상태를 다시 읽는다). */
  refresh: number;
  onChanged: () => void;
}

/** 세 교과 확정본을 합친 통합본(NEIS 「성취수준 및 특기사항」 입력용). */
export default function CombinedPanel({ studentId, from, to, refresh, onChanged }: Props) {
  const key: CombinedKey = useMemo(() => ({ studentId, from, to }), [studentId, from, to]);
  const [view, setView] = useState<CombinedView | null>(null);
  const [text, setText] = useState('');
  const [check, setCheck] = useState<CombinedCheck | null>(null);
  const [busy, setBusy] = useState(false);
  const [askOverwrite, setAskOverwrite] = useState(false);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const dirtyRef = useRef(false);

  const apply = useCallback((v: CombinedView) => {
    setView(v);
    setText(v.combined?.text ?? '');
    setCheck(v.check);
  }, []);

  useEffect(() => {
    let live = true;
    evalApi.combinedView(key).then(
      (v) => {
        if (!live) return;
        // 고치는 중인 글은 교과 평어 갱신으로 잃지 않는다.
        if (dirtyRef.current) {
          setView(v);
          setCheck(v.check);
        } else apply(v);
      },
      (e) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [key, refresh, apply]);

  const saved = view?.combined?.text ?? '';
  const dirty = view?.combined != null && text !== saved;
  dirtyRef.current = dirty;

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
      apply(await evalApi.combinedSave(view.combined.id, text));
      setInfo('저장했습니다.');
      onChanged();
    });

  const confirm = () =>
    run(async () => {
      if (!view?.combined) return;
      if (dirty) await evalApi.combinedSave(view.combined.id, text);
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
      await navigator.clipboard.writeText(saved);
      setInfo('복사했습니다. 나이스 「성취수준 및 특기사항」에 붙여 넣으세요.');
    });

  if (error && !view) return <p className="diag-error">{error}</p>;
  if (!view) return <p className="diag-note">불러오는 중…</p>;

  const c = view.combined;
  const status = c?.status ?? 'none';
  const blocked = check ? check.warnings.filter((w) => w.code === 'forbidden' || w.code === 'otherName') : [];

  return (
    <section className="ev-combined" aria-label={`${view.student.number}번 ${view.student.name} 통합본`}>
      <h3 className="ev-title">
        통합본 (NEIS 입력용)
        <span className={`ev-status ev-status-${status}`}>{status === 'confirmed' ? '확정' : status === 'draft' ? '초안' : '없음'}</span>
      </h3>
      <p className="diag-hint">
        1~2학년 바른 생활·슬기로운 생활·즐거운 생활은 나이스 「성취수준 및 특기사항」에 통합해서 입력합니다. 교과별로 확정한 평어를 합쳐 한 문단으로 다듬어 주세요.
      </p>
      {error && <p className="diag-error">{error}</p>}
      {info && (
        <p className="rec-toast" role="status">
          {info}
        </p>
      )}

      <ul className="ev-subjects" aria-label="교과별 확정 상태">
        {view.subjects.map((s) => (
          <li key={s.code} className={s.confirmed ? 'ev-subject ev-subject-ok' : 'ev-subject'}>
            {s.confirmed ? '✓' : '○'} {s.name} {s.confirmed ? '확정' : '미확정'}
          </li>
        ))}
      </ul>

      <div className="rec-row rec-wrap">
        <button className="diag-primary" disabled={busy || !view.canCombine} onClick={() => (c ? setAskOverwrite(true) : combine(false))}>
          {c ? '세 교과 다시 합치기' : '세 교과 합치기'}
        </button>
        {!view.canCombine && <span className="diag-hint">세 교과 평어를 모두 확정하면 합칠 수 있어요(교과 탭을 바꿔 가며 확정해 주세요).</span>}
        {askOverwrite && (
          <span className="ev-ask" role="alert">
            지금 통합본이 교과 평어를 새로 합친 글로 바뀌고, 고친 내용은 사라집니다. 계속할까요?
            <button className="diag-primary" disabled={busy} onClick={() => combine(true)}>
              다시 합치기
            </button>
            <button className="diag-secondary" onClick={() => setAskOverwrite(false)}>
              취소
            </button>
          </span>
        )}
      </div>

      {c?.stale && (
        <p className="ev-fallback" role="alert">
          교과 평어가 바뀌었습니다. 필요하면 「세 교과 다시 합치기」를 눌러 주세요(지금 통합본은 자동으로 바뀌지 않습니다).
        </p>
      )}

      {c && (
        <>
          <textarea className="rec-textarea ev-combined-text" rows={8} value={text} aria-label="통합본 글" onChange={(e) => setText(e.target.value)} />
          <p className="ev-length" role="status">
            {check ? `${check.chars}자 · NEIS ${check.bytes}바이트` : ''}
            {check?.lengthNotice ? ` — ${check.lengthNotice}` : ''}
          </p>
          {check && check.warnings.length > 0 && (
            <ul className="ev-warns" aria-label="통합본 경고">
              {check.warnings.map((w, i) => (
                <li key={i} className={`ev-warn ev-warn-${w.code}`}>
                  ⚠ {w.message}
                </li>
              ))}
            </ul>
          )}
          {blocked.length > 0 && <p className="diag-hint">금지 표현이나 다른 학생 이름을 고쳐야 확정할 수 있어요.</p>}

          <div className="rec-row rec-wrap ev-actions">
            <button className="diag-secondary" disabled={busy || !dirty} onClick={saveNow}>
              저장
            </button>
            {status === 'confirmed' && !dirty ? (
              <button className="diag-secondary" disabled={busy} onClick={unconfirm}>
                확정 풀기
              </button>
            ) : (
              <button className="diag-primary" disabled={busy || text.trim() === '' || !check?.canConfirm} onClick={confirm}>
                확정
              </button>
            )}
            <button className="diag-primary" disabled={busy || status !== 'confirmed' || dirty} onClick={copy} title={status === 'confirmed' ? '' : '확정한 뒤에 복사할 수 있어요'}>
              복사
            </button>
          </div>
          {status !== 'confirmed' && <p className="diag-hint">확정한 통합본만 복사·내보내기를 할 수 있습니다.</p>}
        </>
      )}
    </section>
  );
}
