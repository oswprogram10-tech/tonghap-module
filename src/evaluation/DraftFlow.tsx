// 교과 평어·행동특성 및 종합의견의 학생 단계 화면(설계 25).
// ① 근거 고르기 → ② 초안 만들기 → ③ 문장 확인(한 번에 한 문장) → ④ 확정 · 복사.
import { useCallback, useEffect, useRef, useState, type MutableRefObject, type ReactNode } from 'react';
import { copyText } from '../lib/clipboard';
import { errorText } from '../records/api';
import { CANCELLED_TEXT, LEN_MAX, LEN_MIN, MIN_EVIDENCE, REVIEW_NOTICE, UNCHANGED_NOTICE, evalApi, generatorLabel, joinSentences, type GeneratorChoice, type Sentence } from './api';
import type { DraftAdapter, FlowCheck, FlowView } from './adapters';
import { evidenceLabel } from './evidenceUi';
import { afterReview, confirmLabel, firstUnreviewed, neisBytes, reviewedCount, startStep, weakReason, type Step } from './stepLogic';
import { EvidenceCard, EvidenceStep, Seg, StepHeader, type StepInfo } from './stepUi';

interface Props {
  adapter: DraftAdapter;
  /** 이 값이 바뀌면 경고를 다시 계산한다(행동특성의 「학기 나누기」). */
  recheckKey?: unknown;
  generator: GeneratorChoice;
  onGenerator: (g: GeneratorChoice) => void;
  llmReady: boolean;
  /** 학급 일괄 생성 중에는 이 학생의 초안을 따로 만들 수 없다. */
  locked: boolean;
  onChanged: () => void;
  onNext: (() => void) | null;
  /** 교과 평어 4단계에서 통합본으로 가는 단추. */
  onGoCombined?: () => void;
  /** 학생을 바꾸기 전에 고친 내용을 저장하게 하는 고리. */
  saveRef: MutableRefObject<(() => Promise<boolean>) | null>;
  /** 2단계에 덧붙일 설정(행동특성의 학기 나누기·목표 글자 수). */
  generateExtra?: (view: FlowView) => ReactNode;
  /** 행동특성이면 「지도가 필요한 모습」 표시 */
  guidanceMarks: boolean;
}

const STEP_TITLES = ['근거 고르기', '초안 만들기', '문장 확인', '확정 · 복사'];
const STATUS_TEXT = { none: '초안 없음', draft: '초안', confirmed: '확정' } as const;

export default function DraftFlow({ adapter, recheckKey, generator, onGenerator, llmReady, locked, onChanged, onNext, onGoCombined, saveRef, generateExtra, guidanceMarks }: Props) {
  const [view, setView] = useState<FlowView | null>(null);
  const [sentences, setSentences] = useState<Sentence[]>([]);
  const [check, setCheck] = useState<FlowCheck | null>(null);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [step, setStep] = useState<Step>(1);
  const [cur, setCur] = useState(0);
  const [busy, setBusy] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [askRegen, setAskRegen] = useState(false);
  const [askUnchanged, setAskUnchanged] = useState(false);
  const [attachOpen, setAttachOpen] = useState(false);
  const [undo, setUndo] = useState<{ index: number; sentence: Sentence } | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const first = useRef(true);

  const apply = useCallback((v: FlowView) => {
    setView(v);
    setSentences(v.sentences);
    setCheck(v.check);
  }, []);

  useEffect(() => {
    let live = true;
    adapter.load().then(
      (v) => {
        if (!live) return;
        apply(v);
        if (first.current) {
          first.current = false;
          const s = startStep(v.status, v.sentences);
          setStep(s.step);
          setCur(s.index);
        }
      },
      (e) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [adapter, apply]);

  const saved = view?.sentences ?? [];
  const dirty = view !== null && JSON.stringify(sentences) !== JSON.stringify(saved);

  // 편집하는 동안 경고를 다시 계산한다(저장하지 않고 검사만).
  useEffect(() => {
    if (!view) return;
    if (sentences.length === 0) {
      setCheck(null);
      return;
    }
    const n = ++seq.current;
    const t = setTimeout(() => {
      adapter.check(sentences).then(
        (c) => n === seq.current && setCheck(c),
        (e) => n === seq.current && setError(errorText(e)),
      );
    }, 250);
    return () => clearTimeout(t);
    // recheckKey: 학기 나누기가 바뀌면 다시 검사
  }, [sentences, adapter, view, recheckKey]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setInfo(null);
    setNotice(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // 학생을 바꾸거나 목록으로 돌아가기 전에 고친 내용을 저장한다.
  useEffect(() => {
    saveRef.current = async () => {
      if (!dirty) return true;
      try {
        apply(await adapter.save(sentences));
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
  }, [dirty, adapter, sentences, apply, onChanged, saveRef]);

  const includedCount = view ? view.bundle.evidence.filter((e) => !excluded.includes(e.id)).length : 0;
  const enough = includedCount >= MIN_EVIDENCE;
  const hasDraft = sentences.length > 0 || saved.length > 0;
  const n = sentences.length;
  const at = Math.min(cur, Math.max(0, n - 1));

  const goStep = (s: Step) => {
    setUndo(null);
    setAttachOpen(false);
    setAskRegen(false);
    // 앞 단계의 안내(예: 「이 초안으로 확인하기」를 누르라는 말)가 다음 단계에 남지 않게 지운다.
    setInfo(null);
    setStep(s);
  };

  const generate = () =>
    run(async () => {
      setAskRegen(false);
      setAskUnchanged(false);
      setUndo(null);
      if (generator === 'llm') {
        setGenerating(true);
        try {
          const r = await adapter.generateLlm(excluded);
          apply(r.view);
          setCur(0);
          if (r.fellBack) setNotice(r.notice);
          else setInfo('언어 모델로 초안을 만들었습니다. 「이 초안으로 확인하기」를 눌러 문장을 읽고 고쳐 주세요.');
          onChanged();
        } catch (e) {
          if (errorText(e) !== CANCELLED_TEXT) throw e;
          setInfo('초안 만들기를 취소했습니다. 기존 문장은 그대로입니다.');
        } finally {
          setGenerating(false);
        }
        return;
      }
      apply(await adapter.generate(excluded));
      setCur(0);
      setInfo('템플릿으로 초안을 만들었습니다. 「이 초안으로 확인하기」를 눌러 문장을 읽고 고쳐 주세요.');
      onChanged();
    });

  const cancelGenerate = () => {
    evalApi.llmCancel().catch((e) => setError(errorText(e)));
  };

  const saveNow = () =>
    run(async () => {
      apply(await adapter.save(sentences));
      setInfo('저장했습니다.');
      onChanged();
    });

  /** 확정. 언어 모델이 만든 글을 한 글자도 고치지 않았으면 먼저 한 번 더 묻는다(막지는 않는다). */
  const confirm = (skipAsk = false) =>
    run(async () => {
      setAskUnchanged(false);
      let v = view;
      if (dirty) {
        v = await adapter.save(sentences);
        apply(v);
      }
      if (!skipAsk && v?.unchangedFromModel) {
        setAskUnchanged(true);
        return;
      }
      apply(await adapter.confirm());
      setInfo('확정했습니다. 이제 복사할 수 있어요.');
      onChanged();
    });

  const unconfirm = () =>
    run(async () => {
      apply(await adapter.save(saved));
      setInfo('확정을 풀었습니다. 고친 뒤 다시 확정해 주세요.');
      onChanged();
    });

  const copy = () =>
    run(async () => {
      await copyText(joinSentences(saved));
      setInfo(adapter.copyDone);
    });

  const setText = (i: number, text: string) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, text } : x)));
  const detach = (i: number, id: string) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, evidence: x.evidence.filter((e) => e !== id) } : x)));
  const attach = (i: number, id: string) => setSentences((s) => s.map((x, j) => (j === i && !x.evidence.includes(id) ? { ...x, evidence: [...x.evidence, id] } : x)));
  const toggleExcluded = (id: string) => setExcluded((x) => (x.includes(id) ? x.filter((y) => y !== id) : [...x, id]));

  const addSentence = () => {
    if (sentences.length >= adapter.maxSentences) return;
    setUndo(null);
    setSentences((s) => [...s, { text: '', evidence: [], reviewed: true }]);
    setCur(sentences.length);
    setStep(3);
  };

  const removeSentence = (i: number) => {
    setUndo({ index: i, sentence: sentences[i] });
    setAttachOpen(false);
    setSentences((s) => s.filter((_, j) => j !== i));
    setCur(Math.max(0, Math.min(i, sentences.length - 2)));
  };

  const undoRemove = () => {
    if (!undo) return;
    const { index, sentence } = undo;
    setSentences((s) => [...s.slice(0, index), sentence, ...s.slice(index)]);
    setCur(index);
    setUndo(null);
  };

  /** 「맞아요, 확인했어요」: 이 문장을 확인한 것으로 하고 다음으로 간다. */
  const review = (i: number) => {
    if (i < 0 || i >= sentences.length) return;
    const dest = afterReview(sentences, i);
    setSentences((s) => s.map((x, j) => (j === i ? { ...x, reviewed: true } : x)));
    setUndo(null);
    setAttachOpen(false);
    setCur(dest.index);
    setStep(dest.step);
  };

  const move = useCallback(
    (d: number) => {
      setUndo(null);
      setAttachOpen(false);
      setCur((c) => Math.max(0, Math.min(sentences.length - 1, Math.min(c, sentences.length - 1) + d)));
    },
    [sentences.length],
  );

  // 키보드(3단계): 글상자 밖에서 Enter = 확인, ←/→ = 이전/다음 문장.
  useEffect(() => {
    if (step !== 3 || !view) return;
    const onKey = (ev: KeyboardEvent) => {
      if (ev.altKey || ev.ctrlKey || ev.metaKey) return;
      const t = ev.target as HTMLElement | null;
      const tag = t?.tagName;
      if (tag === 'TEXTAREA' || tag === 'INPUT' || tag === 'SELECT' || t?.isContentEditable) return;
      if (ev.key === 'ArrowLeft') {
        ev.preventDefault();
        move(-1);
      } else if (ev.key === 'ArrowRight') {
        ev.preventDefault();
        move(1);
      } else if (ev.key === 'Enter' && tag !== 'BUTTON' && tag !== 'A') {
        ev.preventDefault();
        review(Math.min(cur, sentences.length - 1));
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  if (error && !view) return <p className="diag-error">{error}</p>;
  if (!view) return <p className="diag-note">불러오는 중…</p>;

  const status = view.status;
  const byId = new Map(view.bundle.evidence.map((e) => [e.id, e]));
  const done = reviewedCount(sentences);
  const chars = check?.chars ?? 0;
  const bytes = neisBytes(joinSentences(sentences));

  const steps: StepInfo[] = [
    { title: STEP_TITLES[0], summary: `${includedCount}개` },
    { title: STEP_TITLES[1], summary: hasDraft && view.generator ? `${generatorLabel(view.generator)} · ${n}문장` : n > 0 ? `${n}문장` : '아직 없음' },
    { title: STEP_TITLES[2], summary: n > 0 ? `확인 ${done}/${n}` : '문장 없음', disabled: n === 0 },
    { title: STEP_TITLES[3], summary: status === 'confirmed' && !dirty ? '확정함' : 'NEIS에 붙여 넣기', disabled: n === 0 },
  ];
  const doneThrough: Step | 0 = status === 'confirmed' && !dirty ? 4 : ((step - 1) as Step | 0);

  const lengthLine =
    n > 0 ? (
      <p className={check?.length ? 'ev-length ev-warn ev-warn-length' : 'ev-length'}>
        {chars}자 · NEIS {bytes}바이트 (권장 {LEN_MIN}~{LEN_MAX}자, 목표 {view.targetChars}자){check?.length ? ` — ${check.length.message}` : ''}
      </p>
    ) : null;

  return (
    <section className="ev-student es-flow" aria-label={`${view.student.number}번 ${view.student.name} ${adapter.noun}`}>
      <StepHeader steps={steps} current={step} doneThrough={doneThrough} onGo={goStep} />
      <div className="es-statusline">
        <span className={`ev-status ev-status-${status}`}>{STATUS_TEXT[status]}</span>
        <span className="es-muted">
          {adapter.noun}
          {view.generator ? ` · 만든 방식: ${generatorLabel(view.generator)}` : ''}
          {dirty ? ' · 저장하지 않은 고침이 있습니다' : ''}
        </span>
      </div>
      {error && <p className="diag-error">{error}</p>}
      {info && (
        <p className="rec-toast ev-toast" role="status">
          {info}
        </p>
      )}
      {notice && (
        <p className="ev-fallback" role="alert">
          {notice}
        </p>
      )}

      {step === 1 && (
        <EvidenceStep
          bundle={view.bundle}
          excluded={excluded}
          open={open}
          onOpen={setOpen}
          onToggle={toggleExcluded}
          includedCount={includedCount}
          enough={enough}
          emptyText={adapter.emptyEvidence}
          shortText={adapter.shortOfEvidence(includedCount)}
          guidanceMarks={guidanceMarks}
          onNext={() => goStep(2)}
        />
      )}

      {step === 2 && (
        <div className="es-panel">
          <div className="es-panel-head">
            <h3>초안 만들기</h3>
          </div>
          {!enough && (
            <p className="diag-error" role="alert">
              {adapter.shortOfEvidence(includedCount)}
            </p>
          )}
          <div className="es-row">
            <Seg
              label="초안 만드는 방식"
              value={generator}
              disabled={busy || generating}
              onPick={onGenerator}
              options={[
                { value: 'template', label: '템플릿' },
                { value: 'llm', label: '언어 모델', disabled: !llmReady, title: llmReady ? '' : '기기 점검 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요' },
              ]}
            />
            <span className="diag-hint es-grow">
              {llmReady
                ? generator === 'llm'
                  ? '이 PC 안의 언어 모델로 만듭니다. 학생 1명에 30초~1분쯤 걸려요.'
                  : '문장 틀로 바로 만듭니다.'
                : '언어 모델은 「기기 점검」 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요. 지금은 템플릿만 쓸 수 있습니다.'}
            </span>
          </div>
          {generateExtra?.(view)}
          <div className="es-row es-gap">
            {hasDraft ? (
              <>
                <button type="button" className="es-btn es-btn-primary" disabled={n === 0} onClick={() => goStep(3)}>
                  이 초안으로 확인하기
                </button>
                <button type="button" className="es-btn" disabled={busy || locked || !enough || generating} onClick={() => setAskRegen(true)}>
                  다시 만들기
                </button>
              </>
            ) : (
              <>
                <button type="button" className="es-btn es-btn-primary" disabled={busy || locked || !enough} onClick={generate}>
                  초안 만들기
                </button>
                <button type="button" className="es-btn" disabled={busy} onClick={addSentence}>
                  문장을 직접 쓰기
                </button>
              </>
            )}
          </div>
          {askRegen && (
            <div className="es-ask" role="alert">
              <p>지금 문장이 새 초안으로 바뀝니다. 계속할까요?</p>
              <div className="es-row">
                <button type="button" className="es-btn es-btn-primary" disabled={busy} onClick={generate}>
                  덮어쓰고 다시 만들기
                </button>
                <button type="button" className="es-btn" onClick={() => setAskRegen(false)}>
                  취소
                </button>
              </div>
            </div>
          )}
          {generating && (
            <p className="ev-progress" role="status">
              <progress aria-label="초안 만드는 중" />
              초안 만드는 중… 학생 1명에 30초~1분 걸려요.
              <button type="button" className="es-btn" onClick={cancelGenerate}>
                취소
              </button>
            </p>
          )}
          {n === 0 && !generating && <p className="diag-note">아직 문장이 없습니다. 「초안 만들기」를 누르거나 문장을 직접 쓰세요.</p>}
          {n > 0 && lengthLine}
          <div className="es-nav">
            <button type="button" className="es-btn es-btn-quiet" onClick={() => goStep(1)}>
              ← 근거 고르기
            </button>
          </div>
        </div>
      )}

      {step === 3 && n === 0 && (
        <div className="es-panel">
          <p className="diag-note">확인할 문장이 없습니다. 문장을 직접 쓰거나 초안을 만들어 주세요.</p>
          {undo && (
            <p className="es-undo" role="status">
              {undo.index + 1}번 문장을 뺐습니다.
              <button type="button" className="es-btn" onClick={undoRemove}>
                되돌리기
              </button>
            </p>
          )}
          <div className="es-row">
            <button type="button" className="es-btn es-btn-primary" onClick={addSentence}>
              + 문장 추가
            </button>
            <button type="button" className="es-btn" onClick={() => goStep(2)}>
              초안 만들기로
            </button>
          </div>
        </div>
      )}

      {step === 3 && n > 0 && (
        <ReviewStep
          adapter={adapter}
          view={view}
          sentences={sentences}
          check={check}
          at={at}
          byId={byId}
          generatorText={generatorLabel(view.generator ?? 'template')}
          undo={undo}
          attachOpen={attachOpen}
          doneCount={done}
          chars={chars}
          bytes={bytes}
          lengthLine={check?.length?.message ?? null}
          onText={(t) => setText(at, t)}
          onReview={() => review(at)}
          onRemove={() => removeSentence(at)}
          onUndo={undoRemove}
          onMove={move}
          onDetach={(id) => detach(at, id)}
          onAttach={(id) => attach(at, id)}
          onAttachOpen={setAttachOpen}
          onAdd={addSentence}
          onToConfirm={() => goStep(4)}
        />
      )}

      {step === 4 && (
        <ConfirmStep
          adapter={adapter}
          sentences={sentences}
          check={check}
          status={status}
          dirty={dirty}
          busy={busy}
          chars={chars}
          bytes={bytes}
          lengthLine={lengthLine}
          askUnchanged={askUnchanged}
          onText={setText}
          onGoSentence={(i) => {
            setCur(i);
            goStep(3);
          }}
          onSave={saveNow}
          onConfirm={() => confirm()}
          onConfirmAnyway={() => confirm(true)}
          onCancelAsk={() => setAskUnchanged(false)}
          onUnconfirm={unconfirm}
          onCopy={copy}
          onNext={onNext}
          onGoCombined={onGoCombined}
          onBack={() => goStep(3)}
        />
      )}
    </section>
  );
}

interface ReviewProps {
  adapter: DraftAdapter;
  view: FlowView;
  sentences: Sentence[];
  check: FlowCheck | null;
  at: number;
  byId: Map<string, FlowView['bundle']['evidence'][number]>;
  generatorText: string;
  undo: { index: number; sentence: Sentence } | null;
  attachOpen: boolean;
  doneCount: number;
  chars: number;
  bytes: number;
  lengthLine: string | null;
  onText: (t: string) => void;
  onReview: () => void;
  onRemove: () => void;
  onUndo: () => void;
  onMove: (d: number) => void;
  onDetach: (id: string) => void;
  onAttach: (id: string) => void;
  onAttachOpen: (v: boolean) => void;
  onAdd: () => void;
  onToConfirm: () => void;
}

/** 3단계: 한 번에 한 문장. */
function ReviewStep({
  adapter,
  view,
  sentences,
  check,
  at,
  byId,
  generatorText,
  undo,
  attachOpen,
  doneCount,
  chars,
  bytes,
  lengthLine,
  onText,
  onReview,
  onRemove,
  onUndo,
  onMove,
  onDetach,
  onAttach,
  onAttachOpen,
  onAdd,
  onToConfirm,
}: ReviewProps) {
  const s = sentences[at];
  const n = sentences.length;
  const warns = check?.sentences[at] ?? [];
  const weak = weakReason(s.evidence, byId);
  const attachable = view.bundle.evidence.filter((e) => !s.evidence.includes(e.id));
  return (
    <div className="es-review">
      <div className="es-review-grid">
        <section className="es-sentence" aria-label="확인할 문장">
          <div className="es-sentence-meta">
            <span>
              문장 {at + 1} / {n} · 만든 방식: {generatorText}
            </span>
            <span className={s.reviewed ? 'es-chip es-chip-ok' : 'es-chip es-chip-todo'}>{s.reviewed ? '✓ 확인함' : '아직 확인 안 함'}</span>
          </div>
          <label htmlFor="es-sentence-text" className="es-question">
            이 문장이 아이의 실제 모습과 맞나요?
          </label>
          <textarea id="es-sentence-text" className="es-textarea" rows={4} value={s.text} aria-label={`${at + 1}번째 문장`} onChange={(e) => onText(e.target.value)} />
          <p className="es-muted">고치면 고친 글이 그대로 쓰입니다. 글상자 밖에서 Enter = 확인, ← → = 이전·다음 문장.</p>
          {warns.length > 0 && (
            <ul className="ev-warns" aria-label="이 문장의 검사 경고">
              {warns.map((w, k) => (
                <li key={k} className={`ev-warn ev-warn-${w.code}`}>
                  ⚠ {w.message}
                </li>
              ))}
            </ul>
          )}
          <div className="es-actions">
            <button type="button" className="es-btn es-btn-primary es-btn-big" onClick={onReview}>
              맞아요, 확인했어요
            </button>
            <button type="button" className="es-btn" onClick={onRemove}>
              이 문장 빼기
            </button>
            <span className="es-grow" />
            <button type="button" className="es-btn es-btn-quiet" disabled={at === 0} onClick={() => onMove(-1)}>
              ← 이전 문장
            </button>
            <button type="button" className="es-btn es-btn-quiet" disabled={at >= n - 1} onClick={() => onMove(1)}>
              다음 문장 →
            </button>
          </div>
          {undo && (
            <p className="es-undo" role="status">
              {undo.index + 1}번 문장을 뺐습니다.
              <button type="button" className="es-btn" onClick={onUndo}>
                되돌리기
              </button>
            </p>
          )}
          {at === n - 1 && (
            <div className="es-row">
              <button type="button" className="es-btn es-btn-quiet" disabled={n >= adapter.maxSentences} onClick={onAdd}>
                + 문장 추가
              </button>
            </div>
          )}
        </section>

        <aside className="es-evidence" aria-label="이 문장의 근거">
          <h3>이 문장의 근거</h3>
          {weak && (
            <div className="es-weak" role="note">
              <b>⚠ {weak.kind === 'negative' ? '단점 근거가 있습니다' : '근거가 약합니다'}</b>
              <p>{weak.message}</p>
            </div>
          )}
          {s.evidence.map((id) => (
            <EvidenceCard key={id} e={byId.get(id)} bundle={view.bundle} onRemove={() => onDetach(id)} />
          ))}
          <button type="button" className="es-btn es-btn-dashed" aria-expanded={attachOpen} disabled={attachable.length === 0} onClick={() => onAttachOpen(!attachOpen)}>
            + 다른 기록을 근거로 붙이기
          </button>
          {attachOpen && (
            <div className="es-attach" role="group" aria-label="근거로 붙일 기록">
              {attachable.map((e) => (
                <button key={e.id} type="button" className="es-btn es-attach-btn" aria-label={`${evidenceLabel(e)} 근거로 붙이기`} onClick={() => onAttach(e.id)}>
                  {evidenceLabel(e)}
                </button>
              ))}
            </div>
          )}
        </aside>
      </div>

      <section className="es-preview" aria-label="전체 글 미리 보기">
        <div className="es-preview-head">
          <h3>전체 글</h3>
          <span className="es-muted">
            {chars}자 · NEIS {bytes}바이트 · 확인 {doneCount}/{n}
          </span>
          <span className="es-muted es-legend">노란 표시 = 지금 문장 · 연한 초록 밑줄 = 확인한 문장</span>
        </div>
        <p className="es-preview-text">
          {sentences.map((x, i) => {
            const cls = i === at ? 'es-s es-s-now' : x.reviewed ? 'es-s es-s-ok' : 'es-s';
            return i === at ? (
              <mark key={i} className={cls}>
                {x.text.trim()}{' '}
              </mark>
            ) : (
              <span key={i} className={cls}>
                {x.text.trim()}{' '}
              </span>
            );
          })}
        </p>
        {lengthLine && <p className="ev-length ev-warn ev-warn-length">{lengthLine}</p>}
        <div className="es-nav">
          <span className="es-grow" />
          <button type="button" className="es-btn" onClick={onToConfirm}>
            확정 · 복사로 가기
          </button>
        </div>
      </section>
    </div>
  );
}

interface ConfirmProps {
  adapter: DraftAdapter;
  sentences: Sentence[];
  check: FlowCheck | null;
  status: 'none' | 'draft' | 'confirmed';
  dirty: boolean;
  busy: boolean;
  chars: number;
  bytes: number;
  lengthLine: ReactNode;
  askUnchanged: boolean;
  onText: (i: number, t: string) => void;
  onGoSentence: (i: number) => void;
  onSave: () => void;
  onConfirm: () => void;
  onConfirmAnyway: () => void;
  onCancelAsk: () => void;
  onUnconfirm: () => void;
  onCopy: () => void;
  onNext: (() => void) | null;
  onGoCombined?: () => void;
  onBack: () => void;
}

/** 4단계: 확정 · 복사. */
function ConfirmStep({
  adapter,
  sentences,
  check,
  status,
  dirty,
  busy,
  chars,
  bytes,
  lengthLine,
  askUnchanged,
  onText,
  onGoSentence,
  onSave,
  onConfirm,
  onConfirmAnyway,
  onCancelAsk,
  onUnconfirm,
  onCopy,
  onNext,
  onGoCombined,
  onBack,
}: ConfirmProps) {
  const label = confirmLabel({ count: sentences.length, firstUnreviewed: firstUnreviewed(sentences), canConfirm: !!check?.canConfirm });
  const confirmed = status === 'confirmed' && !dirty;
  const warnList = sentences.flatMap((_, i) => (check?.sentences[i] ?? []).map((w) => ({ i, w })));
  return (
    <div className="es-panel es-confirm">
      <div className="es-panel-head">
        <h3>확정 · 복사</h3>
        <span className="es-muted">
          {chars}자 · NEIS {bytes}바이트 · 확인 {reviewedCount(sentences)}/{sentences.length}
        </span>
      </div>
      <p className="diag-hint">전체 글입니다. 고칠 곳이 있으면 여기서 바로 고치세요(고쳐도 그 문장의 확인은 그대로입니다).</p>
      <div className="es-full">
        {sentences.map((s, i) => (
          <textarea
            key={i}
            className="es-textarea es-full-text"
            rows={Math.max(2, Math.ceil(s.text.length / 40))}
            value={s.text}
            aria-label={`${i + 1}번째 문장`}
            onChange={(e) => onText(i, e.target.value)}
          />
        ))}
      </div>
      {lengthLine}
      {check && check.overall.length > 0 && (
        <ul className="ev-warns" aria-label="전체 경고">
          {check.overall.map((w, k) => (
            <li key={k} className={`ev-warn ev-warn-${w.code}`}>
              ⚠ {w.message}
            </li>
          ))}
        </ul>
      )}
      {warnList.length > 0 && (
        <ul className="ev-warns" aria-label="문장별 검사 경고">
          {warnList.map(({ i, w }, k) => (
            <li key={k} className={`ev-warn ev-warn-${w.code}`}>
              ⚠ {i + 1}번 문장: {w.message}{' '}
              <button type="button" className="es-btn es-btn-quiet" onClick={() => onGoSentence(i)}>
                그 문장으로
              </button>
            </li>
          ))}
        </ul>
      )}
      {sentences.length > 0 && check && !check.canConfirm && <p className="diag-hint">문장마다 근거를 하나 이상 붙여야 확정할 수 있어요.</p>}
      <div className="es-actions">
        <button type="button" className="es-btn" disabled={busy || !dirty} onClick={onSave}>
          저장
        </button>
        {confirmed ? (
          <button type="button" className="es-btn" disabled={busy} onClick={onUnconfirm}>
            확정 풀기
          </button>
        ) : (
          <button
            type="button"
            className="es-btn es-btn-primary es-btn-big"
            disabled={busy || label.action === 'none' || label.action === 'blocked'}
            onClick={() => (label.action === 'goto' ? onGoSentence(label.index) : onConfirm())}
          >
            {label.label}
          </button>
        )}
        <button type="button" className="es-btn es-btn-primary" disabled={busy || status !== 'confirmed' || dirty} onClick={onCopy} title={status === 'confirmed' ? '' : '확정한 뒤에 복사할 수 있어요'}>
          {adapter.kind === 'subject' ? '이 교과만 복사 (참고용)' : '복사'}
        </button>
        {confirmed && onNext && (
          <button type="button" className="es-btn es-btn-quiet" onClick={onNext}>
            다음 학생 →
          </button>
        )}
      </div>
      {adapter.kind === 'subject' && (
        <p className="diag-hint">
          NEIS에는 통합본을 붙여 넣습니다. 세 교과가 모두 확정되면 통합본으로 합쳐 주세요.{' '}
          {onGoCombined && (
            <button type="button" className="es-btn es-btn-quiet" onClick={onGoCombined}>
              통합본 단계로
            </button>
          )}
        </p>
      )}
      {status !== 'confirmed' && (
        <p className="diag-hint ev-review-notice" role="note">
          {REVIEW_NOTICE} 확정한 글만 복사·내보내기를 할 수 있습니다.
        </p>
      )}
      {askUnchanged && (
        <div className="ev-ask-box" role="alertdialog" aria-label="그대로 확정할까요">
          <p>{UNCHANGED_NOTICE}</p>
          <div className="es-row">
            <button type="button" className="es-btn es-btn-primary" disabled={busy} onClick={onConfirmAnyway}>
              그대로 확정
            </button>
            <button type="button" className="es-btn" onClick={onCancelAsk}>
              돌아가서 고치기
            </button>
          </div>
        </div>
      )}
      <div className="es-nav">
        <button type="button" className="es-btn es-btn-quiet" onClick={onBack}>
          ← 문장 확인으로
        </button>
      </div>
    </div>
  );
}
