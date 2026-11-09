// 행동특성 및 종합의견 화면(설계 24). 교과 평어와 같은 흐름: 근거 -> 초안 -> 검사 -> 근거 확인 -> 확정·복사.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { copyText } from '../lib/clipboard';
import { saveFile } from '../lib/dialogs';
import { errorText } from '../records/api';
import type { RecordsCtx } from '../records/useRecords';
import {
  BEHAVIOR_TARGET_MAX,
  BEHAVIOR_TARGET_MIN,
  CANCELLED_TEXT,
  MIN_EVIDENCE,
  REVIEW_NOTICE,
  UNCHANGED_NOTICE,
  behaviorApi,
  evalApi,
  generatorLabel,
  joinSentences,
  type BehaviorCheck,
  type BehaviorView,
  type GeneratorChoice,
  type LlmStatus,
  type OpinionKey,
  type Sentence,
  type StudentRow,
} from './api';
import { EvidenceDetail, KIND_LABEL, evidenceLabel } from './StudentDraft';

const STATUS_LABEL = { none: '없음', draft: '초안', confirmed: '확정' } as const;

type Span = 'year' | 1 | 2 | 0;

/** 학년도의 기간: 한 해 전체(3월~이듬해 2월), 1학기(3~8월), 2학기(9월~이듬해 2월). */
function spanPeriod(schoolYear: number, span: 'year' | 1 | 2): { from: string; to: string } {
  if (span === 1) return { from: `${schoolYear}-03-02`, to: `${schoolYear}-08-31` };
  if (span === 2) return { from: `${schoolYear}-09-01`, to: `${schoolYear + 1}-02-28` };
  return { from: `${schoolYear}-03-02`, to: `${schoolYear + 1}-02-28` };
}

interface Props {
  ctx: RecordsCtx;
  llm: LlmStatus | null;
}

export default function BehaviorPanel({ ctx, llm }: Props) {
  const { cls } = ctx;
  const initial = spanPeriod(cls.schoolYear, 'year');
  const [span, setSpan] = useState<Span>('year');
  const [from, setFrom] = useState(initial.from);
  const [to, setTo] = useState(initial.to);
  const [rows, setRows] = useState<StudentRow[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [choice, setChoice] = useState<GeneratorChoice | null>(null);
  const [split, setSplit] = useState(false);
  const [target, setTarget] = useState<string>('300');
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const periodOk = from !== '' && to !== '' && from <= to;
  const llmReady = !!llm && llm.modelReady && llm.engineReady;
  const generator: GeneratorChoice = choice ?? (llmReady ? 'llm' : 'template');

  const reload = useCallback(async () => {
    if (!periodOk) return;
    try {
      setRows(await behaviorApi.status(cls.id, from, to));
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [cls.id, from, to, periodOk]);
  useEffect(() => {
    void reload();
  }, [reload]);

  const pickSpan = (s: 'year' | 1 | 2) => {
    const p = spanPeriod(cls.schoolYear, s);
    setSpan(s);
    setFrom(p.from);
    setTo(p.to);
  };

  const saveTarget = async () => {
    setInfo(null);
    try {
      const n = await behaviorApi.targetSet(Number(target));
      setTarget(String(n));
      setInfo(`목표 글자 수를 ${n}자로 저장했습니다. 다음에 초안을 만들 때부터 적용됩니다.`);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const exportAll = async () => {
    setInfo(null);
    setError(null);
    try {
      const path = await saveFile({
        title: '학급 행동특성 및 종합의견 내보내기',
        defaultPath: `행동특성및종합의견-${cls.schoolYear}-${cls.name}.txt`,
        filters: [{ name: '텍스트 파일', extensions: ['txt'] }],
      });
      if (!path) return;
      const n = await evalApi.exportTxt(cls.id, from, to, path, 'behavior');
      setInfo(`확정된 행동특성 및 종합의견 ${n}건을 저장했습니다: ${path}`);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const confirmedCount = useMemo(() => (rows ?? []).filter((r) => r.status === 'confirmed').length, [rows]);
  const student = rows?.find((r) => r.studentId === selected) ?? null;

  return (
    <>
      <p className="ev-notice" role="note">
        학년 동안 남긴 관찰 기록 전체(교과 무관)에서 근거를 모아 담임교사가 쓰는 「행동특성 및 종합의견」 초안을 만듭니다. 장점과 성장을 중심으로 쓰고, 단점은
        기록된 사실에 근거해 변화 가능성과 함께 쓰세요.
      </p>
      {error && <p className="diag-error">{error}</p>}
      {info && (
        <p className="rec-toast" role="status">
          {info}
        </p>
      )}

      <div className="rec-row rec-wrap ev-controls">
        <div className="rec-chips" role="group" aria-label="기간 선택">
          {(['year', 1, 2] as const).map((s) => (
            <button key={s} className={span === s ? 'rec-chip on' : 'rec-chip'} aria-pressed={span === s} onClick={() => pickSpan(s)}>
              {s === 'year' ? '한 해 전체' : `${s}학기`}
            </button>
          ))}
        </div>
        <label className="rec-field">
          시작일
          <input
            type="date"
            value={from}
            onChange={(e) => {
              setSpan(0);
              setFrom(e.target.value);
            }}
          />
        </label>
        <label className="rec-field">
          끝일
          <input
            type="date"
            value={to}
            onChange={(e) => {
              setSpan(0);
              setTo(e.target.value);
            }}
          />
        </label>
        <button className="diag-secondary" onClick={exportAll} disabled={!periodOk}>
          학급 확정본 내보내기 (.txt)
        </button>
      </div>
      <div className="rec-row rec-wrap ev-controls">
        <div className="rec-chips" role="group" aria-label="초안 만드는 방식">
          <button className={generator === 'template' ? 'rec-chip on' : 'rec-chip'} aria-pressed={generator === 'template'} onClick={() => setChoice('template')}>
            템플릿
          </button>
          <button
            className={generator === 'llm' ? 'rec-chip on' : 'rec-chip'}
            aria-pressed={generator === 'llm'}
            disabled={!llmReady}
            title={llmReady ? '' : '기기 점검 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요'}
            onClick={() => setChoice('llm')}
          >
            언어 모델
          </button>
        </div>
        <label className="ev-review ev-split">
          <input type="checkbox" checked={split} onChange={(e) => setSplit(e.target.checked)} />
          <span>학기 나누기 (1학기·2학기 따로 쓰고 「(1학기)」「(2학기)」를 붙임)</span>
        </label>
        <label className="rec-field">
          목표 글자 수 ({BEHAVIOR_TARGET_MIN}~{BEHAVIOR_TARGET_MAX})
          <input type="number" min={BEHAVIOR_TARGET_MIN} max={BEHAVIOR_TARGET_MAX} step={50} value={target} onChange={(e) => setTarget(e.target.value)} />
        </label>
        <button className="diag-secondary" onClick={saveTarget}>
          글자 수 저장
        </button>
      </div>
      {!periodOk && <p className="diag-error">기간을 올바르게 정해 주세요(시작일이 끝일보다 늦을 수 없습니다).</p>}

      <div className="rec-portfolio ev-layout">
        <aside className="rec-sidebar" aria-label="학생 목록">
          {rows === null && <p className="diag-note">불러오는 중…</p>}
          {rows?.map((r) => (
            <button key={r.studentId} className={selected === r.studentId ? 'rec-side active' : 'rec-side'} onClick={() => setSelected(r.studentId)}>
              <span className="rec-chip-num">{r.number}</span>
              <span className="rec-grow">{r.name}</span>
              <span className="rec-count" title="근거 개수">
                근거 {r.evidenceCount}
              </span>
              <span className={`ev-status ev-status-${r.status}`} title="행동특성 및 종합의견">
                {STATUS_LABEL[r.status]}
              </span>
            </button>
          ))}
          {rows && rows.length > 0 && (
            <p className="diag-hint">
              확정 {confirmedCount}/{rows.length}명
            </p>
          )}
        </aside>
        <div className="rec-main">
          {student && periodOk ? (
            <BehaviorDraft
              key={`${student.studentId}|${from}|${to}`}
              studentId={student.studentId}
              from={from}
              to={to}
              generator={generator}
              split={split}
              onSplit={setSplit}
              onChanged={reload}
            />
          ) : (
            <p className="diag-note">왼쪽에서 학생을 고르면 이 기간의 근거 기록과 행동특성 및 종합의견 초안을 볼 수 있어요.</p>
          )}
        </div>
      </div>
    </>
  );
}

interface DraftProps {
  studentId: string;
  from: string;
  to: string;
  generator: GeneratorChoice;
  split: boolean;
  onSplit: (v: boolean) => void;
  onChanged: () => void;
}

function BehaviorDraft({ studentId, from, to, generator, split, onSplit, onChanged }: DraftProps) {
  const key: OpinionKey = useMemo(() => ({ studentId, from, to }), [studentId, from, to]);
  const [view, setView] = useState<BehaviorView | null>(null);
  const [sentences, setSentences] = useState<Sentence[]>([]);
  const [check, setCheck] = useState<BehaviorCheck | null>(null);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [reviewOpen, setReviewOpen] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [askRegen, setAskRegen] = useState(false);
  const [askUnchanged, setAskUnchanged] = useState(false);
  const [info, setInfo] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  const apply = useCallback(
    (v: BehaviorView) => {
      setView(v);
      setSentences(v.opinion?.sentences ?? []);
      setCheck(v.check);
      if (v.opinion) onSplit(v.opinion.splitSemesters);
    },
    [onSplit],
  );

  useEffect(() => {
    let live = true;
    behaviorApi.view(key).then(
      (v) => live && apply(v),
      (e) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [key, apply]);

  const saved = view?.opinion?.sentences ?? [];
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
      behaviorApi.check(key, sentences, split).then(
        (c) => n === seq.current && setCheck(c),
        (e) => n === seq.current && setError(errorText(e)),
      );
    }, 250);
    return () => clearTimeout(t);
  }, [sentences, key, view, split]);

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
  const hasDraft = saved.length > 0 || sentences.length > 0;
  const allReviewed = sentences.length > 0 && sentences.every((s) => s.reviewed);

  const generate = () =>
    run(async () => {
      setAskRegen(false);
      setAskUnchanged(false);
      if (generator === 'llm') {
        setGenerating(true);
        try {
          const r = await behaviorApi.generateLlm(key, excluded, split);
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
      apply(await behaviorApi.generate(key, excluded, split));
      setInfo('템플릿으로 초안을 만들었습니다. 문장을 읽고 고쳐 주세요.');
      onChanged();
    });

  const saveNow = () =>
    run(async () => {
      apply(await behaviorApi.save(key, sentences, split));
      setInfo('저장했습니다.');
      onChanged();
    });

  /** 확정. 언어 모델이 만든 글을 한 글자도 고치지 않았으면 먼저 한 번 더 묻는다(막지는 않는다). */
  const confirm = (skipAsk = false) =>
    run(async () => {
      setAskUnchanged(false);
      let v = view;
      if (dirty) {
        v = await behaviorApi.save(key, sentences, split);
        apply(v);
      }
      if (!skipAsk && v?.opinion?.unchangedFromModel) {
        setAskUnchanged(true);
        return;
      }
      apply(await behaviorApi.confirm(key));
      setInfo('확정했습니다. 이제 복사할 수 있어요.');
      onChanged();
    });

  const unconfirm = () =>
    run(async () => {
      apply(await behaviorApi.save(key, saved, split));
      setInfo('확정을 풀었습니다. 고친 뒤 다시 확정해 주세요.');
      onChanged();
    });

  const copy = () =>
    run(async () => {
      await copyText(joinSentences(saved));
      setInfo('복사했습니다. 나이스 「행동특성 및 종합의견」에 붙여 넣으세요.');
    });

  const setText = (i: number, text: string) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, text } : x)));
  const detach = (i: number, id: string) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, evidence: x.evidence.filter((e) => e !== id) } : x)));
  const attach = (i: number, id: string) => id && setSentences((s) => s.map((x, j) => (j === i && !x.evidence.includes(id) ? { ...x, evidence: [...x.evidence, id] } : x)));
  const removeSentence = (i: number) => {
    setReviewOpen(null);
    setSentences((s) => s.filter((_, j) => j !== i));
  };
  const toggleReviewed = (i: number) => setSentences((s) => s.map((x, j) => (j === i ? { ...x, reviewed: !x.reviewed } : x)));
  const toggleExcluded = (id: string) => setExcluded((x) => (x.includes(id) ? x.filter((y) => y !== id) : [...x, id]));

  if (error && !view) return <p className="diag-error">{error}</p>;
  if (!view) return <p className="diag-note">불러오는 중…</p>;

  const status = view.opinion?.status ?? 'none';
  const byId = new Map(view.bundle.evidence.map((e) => [e.id, e]));
  const negativeIds = new Set(view.bundle.evidence.filter((e) => e.kind === 'observation' && e.negative).map((e) => e.id));

  return (
    <section className="ev-student ev-behavior" aria-label={`${view.student.number}번 ${view.student.name} 행동특성 및 종합의견`}>
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
          <p className="diag-hint">
            체크를 풀면 초안을 만들 때 쓰지 않습니다. 「지도가 필요한 모습」은 단점 근거이며 변화 가능성과 함께 쓰입니다. 이름은 「학생」, 함께 기록된 친구 이름은 「친구」로 바뀌어 있습니다.
          </p>
          {view.bundle.evidence.length === 0 && <p className="diag-hint">이 기간에 이 아이의 관찰 기록이 없습니다.</p>}
          <div className="rp-list" role="group" aria-label="근거 고르기">
            {view.bundle.evidence.map((e) => {
              const on = !excluded.includes(e.id);
              const pairOf = e.kind === 'observation' ? e.pair : undefined;
              return (
                <div key={e.id} className="ev-ev">
                  <label className={on ? 'rp-pick on' : 'rp-pick'}>
                    <input type="checkbox" checked={on} onChange={() => toggleExcluded(e.id)} aria-label={`${evidenceLabel(e)} 근거로 쓰기`} />
                    <span className="rp-pick-text">
                      <span className="rec-tag">{KIND_LABEL[e.kind]}</span>
                      {e.kind === 'observation' && e.negative && <span className="rec-tag rec-tag-guidance">지도가 필요한 모습</span>} {evidenceLabel(e)}
                      {pairOf && byId.has(pairOf) && <span className="bh-pair">↳ 앞선 「지도가 필요한 모습」 뒤에 이어진 변화 기록</span>}
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
          <h4>행동특성 및 종합의견 초안</h4>
          {!enough && (
            <p className="diag-error" role="alert">
              기록이 부족합니다({includedCount}개). 이 기간의 관찰 기록을 더 남겨 주세요 (초안은 근거 {MIN_EVIDENCE}개부터 만들 수 있어요).
            </p>
          )}
          <div className="rec-row rec-wrap">
            <button className="diag-primary" disabled={busy || !enough} onClick={() => (hasDraft ? setAskRegen(true) : generate())}>
              {hasDraft ? '초안 다시 만들기' : '초안 만들기'} ({generator === 'llm' ? '언어 모델' : '템플릿'}
              {split ? ', 학기 나눔' : ''})
            </button>
            {generating && (
              <span className="ev-progress" role="status">
                <progress aria-label="초안 만드는 중" />
                초안 만드는 중… 학생 1명에 30초~1분 걸려요.
                <button className="diag-secondary" onClick={() => evalApi.llmCancel().catch((e) => setError(errorText(e)))}>
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

          {view.opinion && <p className="diag-hint">만든 방식: {generatorLabel(view.opinion.generator)}</p>}
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
                          {negativeIds.has(id) ? '⚑ ' : ''}
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
                  <div className="rec-row rec-wrap ev-review-row">
                    <label className={s.reviewed ? 'ev-review on' : 'ev-review'}>
                      <input type="checkbox" checked={s.reviewed} onChange={() => toggleReviewed(i)} aria-label={`${i + 1}번째 문장 근거 확인`} />
                      <span>근거 확인</span>
                    </label>
                    <button className="diag-secondary" aria-expanded={reviewOpen === i} onClick={() => setReviewOpen(reviewOpen === i ? null : i)}>
                      {reviewOpen === i ? '근거 기록 접기' : '근거 기록 펼쳐 보기'}
                    </button>
                  </div>
                  {reviewOpen === i && (
                    <div className="ev-review-detail">
                      {s.evidence.length === 0 && <p className="diag-hint">이 문장에는 근거 기록이 없습니다.</p>}
                      {s.evidence.map((id) => {
                        const e = byId.get(id);
                        return e ? <EvidenceDetail key={id} e={e} bundle={view.bundle.standardsText} /> : null;
                      })}
                    </div>
                  )}
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
          <button className="diag-secondary" onClick={() => setSentences((s) => [...s, { text: '', evidence: [], reviewed: true }])} disabled={sentences.length >= 24}>
            문장 추가
          </button>

          {check && check.overall.length > 0 && (
            <ul className="ev-warns" aria-label="전체 경고">
              {check.overall.map((w, k) => (
                <li key={k} className={`ev-warn ev-warn-${w.code}`}>
                  ⚠ {w.message}
                </li>
              ))}
            </ul>
          )}
          <p className={check?.length ? 'ev-length ev-warn ev-warn-length' : 'ev-length'}>
            {sentences.length > 0 && check && (
              <>
                {check.totalChars}자 · NEIS {check.totalBytes}바이트 (목표 {view.targetChars}자)
                {check.length ? ` — ${check.length.message}` : ''}
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
              <button className="diag-primary" disabled={busy || sentences.length === 0 || !check?.canConfirm || !allReviewed} onClick={() => confirm()}>
                확정
              </button>
            )}
            <button className="diag-primary" disabled={busy || status !== 'confirmed' || dirty} onClick={copy} title={status === 'confirmed' ? '' : '확정한 뒤에 복사할 수 있어요'}>
              복사
            </button>
          </div>
          {status !== 'confirmed' && (
            <p className="diag-hint ev-review-notice" role="note">
              {REVIEW_NOTICE}
              {sentences.length > 0 && !allReviewed && ` (「근거 확인」을 누르지 않은 문장 ${sentences.filter((s) => !s.reviewed).length}개)`}
            </p>
          )}
          {askUnchanged && (
            <div className="ev-ask-box" role="alertdialog" aria-label="그대로 확정할까요">
              <p>{UNCHANGED_NOTICE}</p>
              <div className="rec-row rec-wrap">
                <button className="diag-primary" disabled={busy} onClick={() => confirm(true)}>
                  그대로 확정
                </button>
                <button className="diag-secondary" onClick={() => setAskUnchanged(false)}>
                  돌아가서 고치기
                </button>
              </div>
            </div>
          )}
          {status !== 'confirmed' && <p className="diag-hint">확정한 글만 복사·내보내기를 할 수 있습니다.</p>}
        </div>
      </div>
    </section>
  );
}
