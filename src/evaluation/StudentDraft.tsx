import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { copyText } from '../lib/clipboard';
import { errorText } from '../records/api';
import { shorten } from '../records/util';
import { standardByCode } from '../records/util';
import {
  CANCELLED_TEXT,
  LEN_MAX,
  LEN_MIN,
  MIN_EVIDENCE,
  evalApi,
  generatorLabel,
  joinSentences,
  type DraftCheck,
  type DraftKey,
  type DraftView,
  type EvalSettings,
  type Evidence,
  type GeneratorChoice,
  type Sentence,
  type SubjectCode,
} from './api';

const md = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8, 10))}`;

function evidenceLabel(e: Evidence): string {
  if (e.kind === 'observation') return `${md(e.date)} ${shorten(e.text || e.quote || '(메모 없음)', 14)}`;
  if (e.kind === 'habit') return `습관 ${shorten(e.title, 12)}`;
  return `${md(e.date)} 차시 ${shorten(e.title, 12)}`;
}

const KIND_LABEL = { observation: '관찰', habit: '습관', session: '차시' } as const;

interface Props {
  studentId: string;
  subject: SubjectCode;
  from: string;
  to: string;
  settings: EvalSettings;
  /** 초안을 만드는 방식(템플릿 / 언어 모델). */
  generator: GeneratorChoice;
  /** 학급 일괄 생성 중에는 이 학생의 초안을 따로 만들 수 없다. */
  locked: boolean;
  onChanged: () => void;
}

/** 한 아이의 근거 목록과 평어 초안 편집·확정 화면. */
export default function StudentDraft({ studentId, subject, from, to, settings, generator, locked, onChanged }: Props) {
  const key: DraftKey = useMemo(() => ({ studentId, subject, from, to }), [studentId, subject, from, to]);
  const [view, setView] = useState<DraftView | null>(null);
  const [sentences, setSentences] = useState<Sentence[]>([]);
  const [check, setCheck] = useState<DraftCheck | null>(null);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [askRegen, setAskRegen] = useState(false);
  const [info, setInfo] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const seq = useRef(0);

  const apply = useCallback((v: DraftView) => {
    setView(v);
    setSentences(v.draft?.sentences ?? []);
    setCheck(v.check);
  }, []);

  useEffect(() => {
    let live = true;
    evalApi.view(key).then(
      (v) => live && apply(v),
      (e) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [key, apply]);

  const saved = view?.draft?.sentences ?? [];
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
      evalApi.check(key, sentences).then(
        (c) => n === seq.current && setCheck(c),
        (e) => n === seq.current && setError(errorText(e)),
      );
    }, 250);
    return () => clearTimeout(t);
  }, [sentences, key, view]);

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

  const includedCount = view ? view.bundle.evidence.filter((e) => !excluded.includes(e.id)).length : 0;
  const enough = includedCount >= MIN_EVIDENCE;
  const hasDraft = (view?.draft?.sentences.length ?? 0) > 0 || sentences.length > 0;

  const generate = () =>
    run(async () => {
      setAskRegen(false);
      if (generator === 'llm') {
        setGenerating(true);
        try {
          const r = await evalApi.generateLlm(key, excluded);
          apply(r.view);
          if (r.fellBack) setNotice(r.notice);
          else setInfo('언어 모델로 초안을 만들었습니다. 문장을 읽고 고쳐 주세요.');
          onChanged();
        } catch (e) {
          if (errorText(e) !== CANCELLED_TEXT) throw e;
          setInfo('초안 만들기를 취소했습니다. 기존 문장은 그대로입니다.');
        } finally {
          setGenerating(false);
        }
        return;
      }
      const v = await evalApi.generate(key, excluded);
      apply(v);
      setInfo('템플릿으로 초안을 만들었습니다. 문장을 읽고 고쳐 주세요.');
      onChanged();
    });

  const cancelGenerate = () => {
    evalApi.llmCancel().catch((e) => setError(errorText(e)));
  };

  const saveNow = () =>
    run(async () => {
      apply(await evalApi.save(key, sentences));
      setInfo('저장했습니다.');
      onChanged();
    });

  const confirm = () =>
    run(async () => {
      if (dirty) await evalApi.save(key, sentences);
      apply(await evalApi.confirm(key));
      setInfo('확정했습니다. 이제 복사할 수 있어요.');
      onChanged();
    });

  const unconfirm = () =>
    run(async () => {
      apply(await evalApi.save(key, saved));
      setInfo('확정을 풀었습니다. 고친 뒤 다시 확정해 주세요.');
      onChanged();
    });

  const copy = () =>
    run(async () => {
      await copyText(joinSentences(saved));
      setInfo('복사했습니다. 나이스에 붙여 넣으세요.');
    });

  const setText = (i: number, text: string) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, text } : x)));
  const detach = (i: number, id: string) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, evidence: x.evidence.filter((e) => e !== id) } : x)));
  const attach = (i: number, id: string) => id && setSentences((s) => s.map((x, j) => (j === i && !x.evidence.includes(id) ? { ...x, evidence: [...x.evidence, id] } : x)));
  const removeSentence = (i: number) => setSentences((s) => s.filter((_, j) => j !== i));
  const toggleExcluded = (id: string) => setExcluded((x) => (x.includes(id) ? x.filter((y) => y !== id) : [...x, id]));

  if (error && !view) return <p className="diag-error">{error}</p>;
  if (!view) return <p className="diag-note">불러오는 중…</p>;

  const status = view.draft?.status ?? 'none';
  const byId = new Map(view.bundle.evidence.map((e) => [e.id, e]));
  const total = check?.totalChars ?? 0;

  return (
    <section className="ev-student" aria-label={`${view.student.number}번 ${view.student.name} 평어`}>
      <h3 className="ev-title">
        {view.student.number}번 {view.student.name}
        <span className={`ev-status ev-status-${status}`}>{status === 'confirmed' ? '확정' : status === 'draft' ? '초안' : '초안 없음'}</span>
      </h3>
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

      <div className="ev-cols">
        <div className="ev-evidence">
          <h4>
            근거 기록 <span className="rp-count">({includedCount}/{view.bundle.evidence.length})</span>
          </h4>
          <p className="diag-hint">체크를 풀면 초안을 만들 때 쓰지 않습니다. 이름은 「학생」, 함께 기록된 친구 이름은 「친구」로 바뀌어 있습니다.</p>
          {view.bundle.evidence.length === 0 && <p className="diag-hint">이 기간에 이 교과의 기록이 없습니다.</p>}
          <div className="rp-list" role="group" aria-label="근거 고르기">
            {view.bundle.evidence.map((e) => {
              const on = !excluded.includes(e.id);
              return (
                <div key={e.id} className="ev-ev">
                  <label className={on ? 'rp-pick on' : 'rp-pick'}>
                    <input type="checkbox" checked={on} onChange={() => toggleExcluded(e.id)} aria-label={`${evidenceLabel(e)} 근거로 쓰기`} />
                    <span className="rp-pick-text">
                      <span className="rec-tag">{KIND_LABEL[e.kind]}</span> {evidenceLabel(e)}
                    </span>
                  </label>
                  <button className="diag-secondary ev-more" aria-expanded={open === e.id} onClick={() => setOpen(open === e.id ? null : e.id)}>
                    {open === e.id ? '접기' : '기록 보기'}
                  </button>
                  {open === e.id && <EvidenceDetail e={e} bundle={view.bundle.standardsText} />}
                </div>
              );
            })}
          </div>
        </div>

        <div className="ev-draft">
          <h4>평어 초안</h4>
          {!enough && (
            <p className="diag-error" role="alert">
              기록이 부족합니다({includedCount}개). 이 교과 관찰 기록을 더 남겨 주세요 (초안은 근거 {MIN_EVIDENCE}개부터 만들 수 있어요).
            </p>
          )}
          <div className="rec-row rec-wrap">
            <button className="diag-primary" disabled={busy || locked || !enough} onClick={() => (hasDraft ? setAskRegen(true) : generate())}>
              {hasDraft ? '초안 다시 만들기' : '초안 만들기'} ({generator === 'llm' ? '언어 모델' : '템플릿'})
            </button>
            {generating && (
              <span className="ev-progress" role="status">
                <progress aria-label="초안 만드는 중" />
                초안 만드는 중… 학생 1명에 30초~1분 걸려요.
                <button className="diag-secondary" onClick={cancelGenerate}>
                  취소
                </button>
              </span>
            )}
            {askRegen && (
              <span className="ev-ask" role="alert">
                지금 문장이 새 초안으로 바뀝니다. 계속할까요?
                <button className="diag-primary" disabled={busy} onClick={generate}>
                  다시 만들기
                </button>
                <button className="diag-secondary" onClick={() => setAskRegen(false)}>
                  취소
                </button>
              </span>
            )}
          </div>

          {view.draft && <p className="diag-hint">만든 방식: {generatorLabel(view.draft.generator)}</p>}
          {sentences.length === 0 && <p className="diag-note">아직 문장이 없습니다. 「초안 만들기」를 누르거나 문장을 직접 추가하세요.</p>}
          <ol className="ev-sentences">
            {sentences.map((s, i) => {
              const warns = check?.sentences[i] ?? [];
              const attachable = view.bundle.evidence.filter((e) => !s.evidence.includes(e.id));
              return (
                <li key={i} className="ev-sentence">
                  <textarea className="rec-textarea" rows={3} value={s.text} aria-label={`${i + 1}번째 문장`} onChange={(e) => setText(i, e.target.value)} />
                  <div className="rec-chips ev-chips" aria-label={`${i + 1}번째 문장의 근거`}>
                    {s.evidence.map((id) => {
                      const e = byId.get(id);
                      return (
                        <button
                          key={id}
                          className={e ? 'rec-chip ev-chip' : 'rec-chip ev-chip ev-chip-bad'}
                          title="눌러서 근거에서 빼기"
                          aria-label={`근거 ${e ? evidenceLabel(e) : '없는 기록'} 빼기`}
                          onClick={() => detach(i, id)}
                        >
                          {e ? evidenceLabel(e) : '없는 기록'} ✕
                        </button>
                      );
                    })}
                    {attachable.length > 0 && (
                      <select aria-label={`${i + 1}번째 문장에 근거 붙이기`} value="" onChange={(e) => attach(i, e.target.value)}>
                        <option value="">+ 근거 붙이기</option>
                        {attachable.map((e) => (
                          <option key={e.id} value={e.id}>
                            {evidenceLabel(e)}
                          </option>
                        ))}
                      </select>
                    )}
                    <button className="diag-secondary" onClick={() => removeSentence(i)}>
                      문장 지우기
                    </button>
                  </div>
                  {warns.length > 0 && (
                    <ul className="ev-warns">
                      {warns.map((w, k) => (
                        <li key={k} className={`ev-warn ev-warn-${w.code}`}>
                          ⚠ {w.message}
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              );
            })}
          </ol>
          <button className="diag-secondary" onClick={() => setSentences((s) => [...s, { text: '', evidence: [] }])} disabled={sentences.length >= 12}>
            문장 추가
          </button>

          <p className={check?.length ? 'ev-length ev-warn ev-warn-length' : 'ev-length'}>
            {sentences.length > 0 && (
              <>
                {total}자 (권장 {LEN_MIN}~{LEN_MAX}자, 목표 {settings.targetChars}자)
                {check?.length ? ` — ${check.length.message}` : ''}
              </>
            )}
          </p>
          {sentences.length > 0 && check && !check.canConfirm && <p className="diag-hint">문장마다 근거를 하나 이상 붙여야 확정할 수 있어요.</p>}

          <div className="rec-row rec-wrap ev-actions">
            <button className="diag-secondary" disabled={busy || !dirty} onClick={saveNow}>
              저장
            </button>
            {status === 'confirmed' && !dirty ? (
              <button className="diag-secondary" disabled={busy} onClick={unconfirm}>
                확정 풀기
              </button>
            ) : (
              <button className="diag-primary" disabled={busy || sentences.length === 0 || !check?.canConfirm} onClick={confirm}>
                확정
              </button>
            )}
            <button className="diag-primary" disabled={busy || status !== 'confirmed' || dirty} onClick={copy} title={status === 'confirmed' ? '' : '확정한 뒤에 복사할 수 있어요'}>
              이 교과만 복사
            </button>
          </div>
          <p className="diag-hint">NEIS에는 통합본을 붙여 넣습니다(아래 「통합본」). 이 교과만 복사는 참고용입니다.</p>
          {status !== 'confirmed' && <p className="diag-hint">확정한 평어만 복사·내보내기를 할 수 있습니다.</p>}
        </div>
      </div>
    </section>
  );
}

function EvidenceDetail({ e, bundle }: { e: Evidence; bundle: Record<string, string> }) {
  const stds = e.kind === 'habit' ? [] : e.standards;
  return (
    <div className="ev-detail">
      {e.kind === 'observation' && (
        <>
          <p>
            {e.date} · {e.topic ?? '주제책 없음'}
            {e.together && ' · 친구와 함께 기록된 관찰'}
          </p>
          {e.text && <p className="ev-quote-text">{e.text}</p>}
          {e.quote && <p className="ev-quote-text">음성 메모 자막: “{e.quote}”</p>}
          {e.tags.length > 0 && <p>태그: {e.tags.join(', ')}</p>}
        </>
      )}
      {e.kind === 'habit' && (
        <p>
          「{e.title}」 {e.schoolDays}일 중 {e.doneDays}일 실천
        </p>
      )}
      {e.kind === 'session' && (
        <p>
          {e.date} · {e.topic ?? '주제책 없음'} · {e.title}
        </p>
      )}
      {stds.map((c) => (
        <p key={c} className="diag-hint">
          <code>{c}</code> {bundle[c] ?? standardByCode(c)?.standard.statement ?? ''}
        </p>
      ))}
    </div>
  );
}
