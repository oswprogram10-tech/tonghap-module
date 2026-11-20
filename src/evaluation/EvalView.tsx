// 평어 탭(설계 25): 처음에는 학생 목록, 학생을 누르면 4단계 화면.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { saveFile } from '../lib/dialogs';
import { errorText, type ClassInfo } from '../records/api';
import ClassManager from '../records/ClassManager';
import { RecordsGate, type RecordsCtx } from '../records/useRecords';
import {
  CANCELLED_TEXT,
  LEN_MAX,
  LEN_MIN,
  MIN_EVIDENCE,
  STYLE_LABEL,
  SUBJECT_LIST,
  behaviorApi,
  currentSemester,
  evalApi,
  semesterPeriod,
  type CombinedRow,
  type EvalSettings,
  type EvalStyle,
  type ExportFormat,
  type GeneratorChoice,
  type LlmStatus,
  type StudentRow,
  type SubjectCode,
} from './api';
import CombinedFlow from './CombinedFlow';
import { BehaviorFlow, SubjectFlow } from './flows';
import { dotDate, nextUnconfirmed } from './stepLogic';
import { Seg } from './stepUi';

type Mode = 'subject' | 'combined' | 'behavior';
type Span = 'year' | 1 | 2 | 0;
type Panel = null | 'style' | 'target';

const STATUS_LABEL = { none: '없음', draft: '초안', confirmed: '확정' } as const;
const SUBJECT_COLOR: Record<SubjectCode, string> = { 바: 'var(--bareun)', 슬: 'var(--seulgi)', 즐: 'var(--jeulgeo)' };
const MODE_TITLE: Record<Mode, string> = { subject: '', combined: '통합본', behavior: '행동특성 및 종합의견' };

/** 행동특성의 기간: 한 해 전체(3월~이듬해 2월), 1학기(3~8월), 2학기(9월~이듬해 2월). */
function spanPeriod(schoolYear: number, span: 'year' | 1 | 2): { from: string; to: string } {
  if (span === 1) return { from: `${schoolYear}-03-02`, to: `${schoolYear}-08-31` };
  if (span === 2) return { from: `${schoolYear}-09-01`, to: `${schoolYear + 1}-02-28` };
  return { from: `${schoolYear}-03-02`, to: `${schoolYear + 1}-02-28` };
}

interface Batch {
  running: boolean;
  total: number;
  done: number;
  current: string;
  /** 취소해서 멈췄는가 */
  cancelled: boolean;
  failures: { name: string; reason: string }[];
  /** 언어 모델 초안을 만들지 못해 템플릿으로 대신한 학생 */
  fallbacks: string[];
}

export default function EvalView() {
  return <RecordsGate what="평어 초안">{(ctx) => <EvalPanel key={ctx.cls.id} ctx={ctx} />}</RecordsGate>;
}

function EvalPanel({ ctx }: { ctx: RecordsCtx }) {
  const { cls, classes } = ctx;
  const [mode, setMode] = useState<Mode>('subject');
  const [subject, setSubject] = useState<SubjectCode>('슬');
  const [semester, setSemester] = useState<1 | 2 | 0>(currentSemester());
  const initial = semesterPeriod(cls.schoolYear, currentSemester());
  const [sFrom, setSFrom] = useState(initial.from);
  const [sTo, setSTo] = useState(initial.to);
  const bInitial = spanPeriod(cls.schoolYear, 'year');
  const [span, setSpan] = useState<Span>('year');
  const [bFrom, setBFrom] = useState(bInitial.from);
  const [bTo, setBTo] = useState(bInitial.to);
  const [split, setSplit] = useState(false);
  const [rows, setRows] = useState<StudentRow[] | null>(null);
  const [combinedRows, setCombinedRows] = useState<Record<string, CombinedRow>>({});
  const [subjRows, setSubjRows] = useState<Record<SubjectCode, Record<string, StudentRow['status']>> | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [settings, setSettings] = useState<EvalSettings | null>(null);
  const [panel, setPanel] = useState<Panel>(null);
  const [periodOpen, setPeriodOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [managing, setManaging] = useState(false);
  const [llm, setLlm] = useState<LlmStatus | null>(null);
  const [choice, setChoice] = useState<GeneratorChoice | null>(null);
  const [batch, setBatch] = useState<Batch | null>(null);
  const [rev, setRev] = useState(0);
  const stopBatch = useRef(false);
  const batchRunning = useRef(false);
  const saveRef = useRef<(() => Promise<boolean>) | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const behavior = mode === 'behavior';
  const from = behavior ? bFrom : sFrom;
  const to = behavior ? bTo : sTo;
  const periodOk = from !== '' && to !== '' && from <= to;

  const reload = useCallback(async () => {
    if (!periodOk) return;
    try {
      if (mode === 'behavior') {
        setRows(await behaviorApi.status(cls.id, from, to));
      } else {
        setRows(await evalApi.status(cls.id, subject, from, to));
        setCombinedRows(await evalApi.combinedStatus(cls.id, from, to));
        if (mode === 'combined') {
          const all = await Promise.all(SUBJECT_LIST.map((s) => evalApi.status(cls.id, s.code, from, to)));
          const map = {} as Record<SubjectCode, Record<string, StudentRow['status']>>;
          SUBJECT_LIST.forEach((s, i) => {
            map[s.code] = Object.fromEntries(all[i].map((r) => [r.studentId, r.status]));
          });
          setSubjRows(map);
        }
      }
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [cls.id, mode, subject, from, to, periodOk]);
  useEffect(() => {
    void reload();
  }, [reload]);
  useEffect(() => {
    evalApi.settingsGet().then(setSettings, (e) => setError(errorText(e)));
    evalApi.llmStatus().then(setLlm, () => setLlm(null));
  }, []);
  // 탭을 떠나면 일괄 생성을 멈춘다.
  useEffect(
    () => () => {
      if (stopBatch.current === false && batchRunning.current) {
        stopBatch.current = true;
        void evalApi.llmCancel().catch(() => undefined);
      }
    },
    [],
  );

  const llmReady = !!llm && llm.modelReady && llm.engineReady;
  const generator: GeneratorChoice = choice ?? (llmReady ? 'llm' : 'template');
  const running = batch?.running ?? false;

  const pickSemester = (s: 1 | 2) => {
    const p = semesterPeriod(cls.schoolYear, s);
    setSemester(s);
    setSFrom(p.from);
    setSTo(p.to);
  };
  const pickSpan = (s: 'year' | 1 | 2) => {
    const p = spanPeriod(cls.schoolYear, s);
    setSpan(s);
    setBFrom(p.from);
    setBTo(p.to);
  };
  const periodName = behavior ? (span === 'year' ? '한 해 전체' : span === 0 ? '기간 지정' : `${span}학기`) : semester === 0 ? '기간 지정' : `${semester}학기`;
  const semesterName = behavior ? periodName : periodName;

  const exportAll = async (format: ExportFormat) => {
    setMenuOpen(false);
    setInfo(null);
    setError(null);
    try {
      const isBehavior = format === 'behavior';
      const path = await saveFile({
        title: isBehavior ? '학급 행동특성 및 종합의견 내보내기' : '학급 평어 내보내기',
        defaultPath: isBehavior ? `행동특성및종합의견-${cls.schoolYear}-${cls.name}.txt` : `평어-${cls.schoolYear}-${cls.name}.txt`,
        filters: [{ name: '텍스트 파일', extensions: ['txt'] }],
      });
      if (!path) return;
      const n = await evalApi.exportTxt(cls.id, from, to, path, format);
      setInfo(
        isBehavior
          ? `확정된 행동특성 및 종합의견 ${n}건을 저장했습니다: ${path}`
          : `확정된 ${format === 'combined' ? '통합본' : '교과별 평어'} ${n}건을 저장했습니다: ${path}`,
      );
    } catch (e) {
      setError(errorText(e));
    }
  };

  const batchTargets = useMemo(() => (mode === 'subject' ? (rows ?? []).filter((r) => r.status === 'none' && r.evidenceCount >= MIN_EVIDENCE) : []), [rows, mode]);

  /** 아직 초안이 없는 학생만, 하나씩 순서대로 만든다. */
  const runBatch = async () => {
    const targets = batchTargets;
    if (targets.length === 0 || running) return;
    const method = generator;
    stopBatch.current = false;
    batchRunning.current = true;
    setInfo(null);
    setError(null);
    const state: Batch = { running: true, total: targets.length, done: 0, current: targets[0].name, cancelled: false, failures: [], fallbacks: [] };
    setBatch({ ...state });
    for (const t of targets) {
      if (stopBatch.current) break;
      setBatch({ ...state, current: `${t.number}번 ${t.name}` });
      const key = { studentId: t.studentId, subject, from, to };
      try {
        if (method === 'llm') {
          const r = await evalApi.generateLlm(key, []);
          if (r.fellBack) state.fallbacks.push(`${t.number}번 ${t.name}`);
        } else {
          await evalApi.generate(key, []);
        }
      } catch (e) {
        const msg = errorText(e);
        if (msg === CANCELLED_TEXT || stopBatch.current) break;
        state.failures.push({ name: `${t.number}번 ${t.name}`, reason: msg });
      }
      state.done += 1;
      setBatch({ ...state });
      void reload();
    }
    state.cancelled = stopBatch.current;
    batchRunning.current = false;
    setBatch({ ...state, running: false, cancelled: state.cancelled });
    setRev((n) => n + 1);
    await reload();
  };

  const cancelBatch = () => {
    stopBatch.current = true;
    if (generator === 'llm') void evalApi.llmCancel().catch(() => undefined);
  };

  const statusOf = useCallback((r: StudentRow) => (mode === 'combined' ? combinedRows[r.studentId]?.status ?? 'none' : r.status), [mode, combinedRows]);
  const counts = useMemo(() => {
    const c = { confirmed: 0, draft: 0, none: 0 };
    for (const r of rows ?? []) c[statusOf(r)] += 1;
    return c;
  }, [rows, statusOf]);
  const items = useMemo(() => (rows ?? []).map((r) => ({ id: r.studentId, done: statusOf(r) === 'confirmed' })), [rows, statusOf]);

  const index = rows ? rows.findIndex((r) => r.studentId === selected) : -1;
  const student = index >= 0 && rows ? rows[index] : null;
  const nextId = selected ? nextUnconfirmed(items, selected) : null;

  /** 화면을 옮기기 전에 고친 내용을 저장한다. 저장하지 못하면 옮기지 않는다. */
  const guard = async (fn: () => void) => {
    const save = saveRef.current;
    if (save && !(await save())) return;
    fn();
  };
  const openStudent = (id: string) => {
    setError(null);
    setInfo(null);
    setSelected(id);
  };
  const backToList = () =>
    guard(() => {
      setPickerOpen(false);
      setSelected(null);
      void reload();
    });
  const goNext = nextId ? () => void guard(() => setSelected(nextId)) : null;

  const switchMode = (m: Mode) => {
    setMode(m);
    setMenuOpen(false);
    setPeriodOpen(false);
    setPanel(null);
    setInfo(null);
    setError(null);
  };

  const header = (
    <div className="es-top">
      <h2 className="es-title">평어</h2>
      {classes.length > 1 && (
        <label className="rec-field es-classpick">
          학급
          <select
            value={cls.id}
            onChange={(e) => {
              setSelected(null);
              ctx.selectClass(e.target.value);
            }}
          >
            {classes.map((c: ClassInfo) => (
              <option key={c.id} value={c.id}>
                {c.schoolYear}학년도 {c.grade}학년 {c.name}
              </option>
            ))}
          </select>
        </label>
      )}
      <button type="button" className="es-btn" onClick={() => setManaging((m) => !m)}>
        학급 관리
      </button>
    </div>
  );

  const managerBlock = managing && (
    <ClassManager
      classes={classes}
      onChanged={async () => {
        await ctx.reloadClasses();
        await reload();
      }}
      onClose={() => setManaging(false)}
    />
  );

  // ---------- 학생 단계 화면 ----------
  if (selected && student && settings && periodOk) {
    const common = {
      studentId: student.studentId,
      from,
      to,
      generator,
      onGenerator: (g: GeneratorChoice) => setChoice(g),
      llmReady,
      locked: running,
      onChanged: reload,
      onNext: goNext,
      saveRef,
    };
    const title = mode === 'subject' ? SUBJECT_LIST.find((s) => s.code === subject)?.name : MODE_TITLE[mode];
    const key = `${mode}|${subject}|${student.studentId}|${from}|${to}|${rev}`;
    return (
      <main className="rec es-screen">
        <header className="es-head">
          <button type="button" className="es-btn es-btn-quiet" onClick={() => void backToList()}>
            ← 평어 목록
          </button>
          <div className="es-head-title">
            {student.name} · {title} · {semesterName}
          </div>
          <span className="es-grow" />
          <div className="es-picker">
            <button type="button" className="es-btn" aria-haspopup="listbox" aria-expanded={pickerOpen} onClick={() => setPickerOpen((v) => !v)}>
              학생 {index + 1}/{rows?.length ?? 0} ▾
            </button>
            {pickerOpen && (
              <ul className="es-pop" role="listbox" aria-label="학생 고르기">
                {rows?.map((r) => (
                  <li key={r.studentId} role="option" aria-selected={r.studentId === selected}>
                    <button
                      type="button"
                      className={r.studentId === selected ? 'es-pop-item on' : 'es-pop-item'}
                      onClick={() =>
                        void guard(() => {
                          setPickerOpen(false);
                          setSelected(r.studentId);
                        })
                      }
                    >
                      <span className="es-num">{r.number}</span>
                      <span className="es-grow">{r.name}</span>
                      <span className={`ev-status ev-status-${statusOf(r)}`}>{STATUS_LABEL[statusOf(r)]}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <button type="button" className="es-btn es-btn-link" disabled={!goNext} title={goNext ? '' : '확정하지 않은 다른 학생이 없습니다'} onClick={() => goNext?.()}>
            다음 학생 →
          </button>
        </header>
        {error && <p className="diag-error">{error}</p>}
        <div key={key}>
          {mode === 'subject' && <SubjectFlow {...common} subject={subject} settings={settings} onGoCombined={() => void guard(() => setMode('combined'))} />}
          {mode === 'behavior' && <BehaviorFlow {...common} split={split} onSplit={setSplit} />}
          {mode === 'combined' && (
            <CombinedFlow
              studentId={student.studentId}
              from={from}
              to={to}
              onChanged={reload}
              onNext={goNext}
              saveRef={saveRef}
              onGoSubject={(code) =>
                void guard(() => {
                  setSubject(code);
                  setMode('subject');
                })
              }
            />
          )}
        </div>
      </main>
    );
  }

  // ---------- 목록 화면 ----------
  return (
    <main className="rec es-screen">
      {header}
      {managerBlock}

      <div className="es-toolbar">
        <Seg<Mode>
          label="평어 종류"
          value={mode}
          disabled={running}
          onPick={switchMode}
          options={[
            { value: 'subject', label: '교과 · 성취수준 및 특기사항' },
            { value: 'combined', label: '통합본' },
            { value: 'behavior', label: '행동특성 및 종합의견' },
          ]}
        />
        <span className="es-grow" />
        {mode === 'subject' && (
          <button type="button" className="es-btn es-btn-primary" onClick={runBatch} disabled={running || !periodOk || batchTargets.length === 0}>
            학급 초안 한꺼번에 만들기 ({batchTargets.length}명)
          </button>
        )}
        <div className="es-menu">
          <button type="button" className="es-btn es-btn-icon" aria-label="더 보기: 내보내기, 문체·금지 표현 설정, 목표 글자 수" aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>
            ⋯
          </button>
          {menuOpen && (
            <ul className="es-pop es-pop-right" role="menu" aria-label="더 보기">
              {mode === 'behavior' ? (
                <li role="none">
                  <button type="button" role="menuitem" className="es-pop-item" disabled={!periodOk} onClick={() => void exportAll('behavior')}>
                    행동특성 및 종합의견 내보내기 (.txt)
                  </button>
                </li>
              ) : (
                <>
                  <li role="none">
                    <button type="button" role="menuitem" className="es-pop-item" disabled={!periodOk} onClick={() => void exportAll('combined')}>
                      통합본(NEIS용) 내보내기 (.txt)
                    </button>
                  </li>
                  <li role="none">
                    <button type="button" role="menuitem" className="es-pop-item" disabled={!periodOk} onClick={() => void exportAll('bySubject')}>
                      교과별 내보내기 (.txt)
                    </button>
                  </li>
                </>
              )}
              <li role="none">
                <button
                  type="button"
                  role="menuitem"
                  className="es-pop-item"
                  onClick={() => {
                    setPanel(panel === 'style' ? null : 'style');
                    setMenuOpen(false);
                  }}
                >
                  문체·금지 표현 설정
                </button>
              </li>
              <li role="none">
                <button
                  type="button"
                  role="menuitem"
                  className="es-pop-item"
                  onClick={() => {
                    setPanel(panel === 'target' ? null : 'target');
                    setMenuOpen(false);
                  }}
                >
                  목표 글자 수
                </button>
              </li>
            </ul>
          )}
        </div>
      </div>

      <div className="es-toolbar">
        {mode === 'subject' && (
          <Seg<SubjectCode>
            label="교과 선택"
            value={subject}
            disabled={running}
            onPick={setSubject}
            options={SUBJECT_LIST.map((s) => ({
              value: s.code,
              label: (
                <>
                  <span className="es-dot" style={{ background: SUBJECT_COLOR[s.code] }} aria-hidden="true" />
                  {s.name}
                </>
              ),
            }))}
          />
        )}
        <button type="button" className="es-btn" aria-expanded={periodOpen} aria-controls="es-period-panel" disabled={running} onClick={() => setPeriodOpen((v) => !v)}>
          {periodName} · {dotDate(from)} ~ {dotDate(to)} ▾
        </button>
        {mode !== 'combined' && (
          <Seg<GeneratorChoice>
            label="초안 만드는 방식"
            value={generator}
            disabled={running}
            onPick={setChoice}
            options={[
              { value: 'template', label: '템플릿' },
              { value: 'llm', label: '언어 모델', disabled: !llmReady, title: llmReady ? '' : '기기 점검 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요' },
            ]}
          />
        )}
      </div>
      {mode !== 'combined' && (
        <p className="diag-hint es-gen-hint">
          {llmReady
            ? generator === 'llm'
              ? '이 PC 안의 언어 모델로 만듭니다. 학생 1명에 30초~1분쯤 걸려요.'
              : '문장 틀로 바로 만듭니다.'
            : '언어 모델은 「기기 점검」 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요. 지금은 템플릿만 쓸 수 있습니다.'}
        </p>
      )}

      {periodOpen && (
        <section id="es-period-panel" className="diag-section es-period" aria-label="기간 정하기">
          {behavior ? (
            <Seg<Span> label="기간 선택" value={span} disabled={running} onPick={(v) => v !== 0 && pickSpan(v)} options={[{ value: 'year', label: '한 해 전체' }, { value: 1, label: '1학기' }, { value: 2, label: '2학기' }]} />
          ) : (
            <Seg<1 | 2 | 0> label="학기 선택" value={semester} disabled={running} onPick={(v) => v !== 0 && pickSemester(v)} options={[{ value: 1, label: '1학기' }, { value: 2, label: '2학기' }]} />
          )}
          <label className="rec-field">
            시작일
            <input
              type="date"
              disabled={running}
              value={from}
              onChange={(e) => {
                if (behavior) {
                  setSpan(0);
                  setBFrom(e.target.value);
                } else {
                  setSemester(0);
                  setSFrom(e.target.value);
                }
              }}
            />
          </label>
          <label className="rec-field">
            끝일
            <input
              type="date"
              disabled={running}
              value={to}
              onChange={(e) => {
                if (behavior) {
                  setSpan(0);
                  setBTo(e.target.value);
                } else {
                  setSemester(0);
                  setSTo(e.target.value);
                }
              }}
            />
          </label>
          <button type="button" className="es-btn" onClick={() => setPeriodOpen(false)}>
            닫기
          </button>
        </section>
      )}
      {!periodOk && <p className="diag-error">기간을 올바르게 정해 주세요(시작일이 끝일보다 늦을 수 없습니다).</p>}

      <p className="es-notice" role="note">
        초안입니다. 교사의 관찰과 판단으로 고쳐 주세요. 확정해야 복사·내보내기를 할 수 있습니다.
        {behavior && ' 장점과 성장을 중심으로 쓰고, 단점은 기록된 사실에 근거해 변화 가능성과 함께 쓰세요.'}
      </p>
      {error && <p className="diag-error">{error}</p>}
      {info && (
        <p className="rec-toast" role="status">
          {info}
        </p>
      )}
      {batch && <BatchPanel batch={batch} onCancel={cancelBatch} onClose={() => setBatch(null)} />}
      {panel && settings && (
        <SettingsPanel
          part={panel}
          settings={settings}
          onSaved={(s) => {
            setSettings(s);
            setInfo('설정을 저장했습니다');
          }}
          onError={(m) => setError(m)}
          onClose={() => setPanel(null)}
        />
      )}

      <section className="es-list" aria-label="학생 목록">
        <div className="es-stats" role="group" aria-label="확정 현황">
          <div className="es-stat es-stat-ok">
            <b>{counts.confirmed}</b>
            <span>확정</span>
          </div>
          <div className="es-stat es-stat-draft">
            <b>{counts.draft}</b>
            <span>초안</span>
          </div>
          <div className="es-stat">
            <b>{counts.none}</b>
            <span>없음</span>
          </div>
        </div>
        {rows === null && <p className="diag-note">불러오는 중…</p>}
        {rows && rows.length === 0 && <p className="diag-note">이 학급에 학생이 없습니다.</p>}
        {rows && rows.length > 0 && (
          <ul className="es-rows">
            {mode === 'combined' ? (
              <li className="es-row-head" aria-hidden="true">
                <span className="es-c-num">번호</span>
                <span className="es-c-name">이름</span>
                <span className="es-c-subj">세 교과 확정</span>
                <span className="es-c-stat">통합본</span>
              </li>
            ) : (
              <li className="es-row-head" aria-hidden="true">
                <span className="es-c-num">번호</span>
                <span className="es-c-name">이름</span>
                <span className="es-c-stat">상태</span>
                <span className="es-c-prog">확인 진행</span>
                {mode === 'subject' && <span className="es-c-comb">통합본</span>}
              </li>
            )}
            {rows.map((r) => (
              <li key={r.studentId}>
                <button type="button" className="es-row-btn" onClick={() => openStudent(r.studentId)}>
                  <span className="es-c-num">{r.number}</span>
                  <span className="es-c-name">{r.name}</span>
                  {mode === 'combined' ? (
                    <>
                      <span className="es-c-subj">
                        {SUBJECT_LIST.map((s) => {
                          const ok = subjRows?.[s.code]?.[r.studentId] === 'confirmed';
                          return (
                            <span key={s.code} className={ok ? 'es-mark es-mark-ok' : 'es-mark'} title={`${s.name} ${ok ? '확정' : '미확정'}`}>
                              {s.code} {ok ? '✓' : '○'}
                              <span className="es-sr">{ok ? ' 확정' : ' 미확정'}</span>
                            </span>
                          );
                        })}
                      </span>
                      <span className="es-c-stat">
                        <span className={`ev-status ev-status-${statusOf(r)}`}>{STATUS_LABEL[statusOf(r)]}</span>
                        {combinedRows[r.studentId]?.stale ? <span className="es-stale">바뀜</span> : null}
                      </span>
                    </>
                  ) : (
                    <>
                      <span className="es-c-stat">
                        <span className={`ev-status ev-status-${r.status}`}>{STATUS_LABEL[r.status]}</span>
                      </span>
                      <span className="es-c-prog">{progressText(r)}</span>
                      {mode === 'subject' && (
                        <span className="es-c-comb">
                          <span className={`ev-status ev-status-${combinedRows[r.studentId]?.status ?? 'none'}`} title="통합본">
                            통합 {STATUS_LABEL[combinedRows[r.studentId]?.status ?? 'none']}
                            {combinedRows[r.studentId]?.stale ? '·바뀜' : ''}
                          </span>
                        </span>
                      )}
                    </>
                  )}
                  <span className="es-c-go" aria-hidden="true">
                    ›
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}

function progressText(r: StudentRow): string {
  if (r.status === 'confirmed') return `확정 완료 · 근거 ${r.evidenceCount}개`;
  if (r.status === 'draft') return `문장 확인 중 · 근거 ${r.evidenceCount}개`;
  return r.evidenceCount >= MIN_EVIDENCE ? `초안 만들기 전 · 근거 ${r.evidenceCount}개` : `기록 부족 · 근거 ${r.evidenceCount}개`;
}

function BatchPanel({ batch, onCancel, onClose }: { batch: Batch; onCancel: () => void; onClose: () => void }) {
  return (
    <section className="diag-section ev-batch" aria-label="학급 초안 만들기">
      {batch.running ? (
        <>
          <p role="status">
            초안 만드는 중… {batch.done}/{batch.total}명 · 지금 {batch.current}
          </p>
          <progress max={batch.total} value={batch.done} aria-label="학급 초안 진행" />
          <p className="diag-hint">하나씩 차례로 만듭니다. 학생 1명에 30초~1분쯤 걸리며, 취소해도 이미 만든 초안은 남습니다.</p>
          <button className="diag-secondary" onClick={onCancel}>
            취소
          </button>
        </>
      ) : (
        <>
          <p role="status">
            {batch.cancelled ? '취소했습니다. ' : '끝났습니다. '}
            {batch.done}/{batch.total}명의 초안을 만들었습니다.
          </p>
          {batch.fallbacks.length > 0 && (
            <p className="ev-fallback" role="alert">
              언어 모델 초안을 만들지 못해 템플릿 초안으로 대신한 학생: {batch.fallbacks.join(', ')}
            </p>
          )}
          {batch.failures.length > 0 && (
            <div role="alert">
              <p className="diag-error">만들지 못한 학생 {batch.failures.length}명</p>
              <ul className="ev-fail-list">
                {batch.failures.map((f) => (
                  <li key={f.name}>
                    {f.name}: {f.reason}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <button className="diag-secondary" onClick={onClose}>
            닫기
          </button>
        </>
      )}
    </section>
  );
}

function SettingsPanel({ part, settings, onSaved, onError, onClose }: { part: 'style' | 'target'; settings: EvalSettings; onSaved: (s: EvalSettings) => void; onError: (m: string) => void; onClose: () => void }) {
  const [style, setStyle] = useState<EvalStyle>(settings.style);
  const [target, setTarget] = useState(String(settings.targetChars));
  const [extra, setExtra] = useState(settings.extraForbidden.join('\n'));
  const run = async () => {
    try {
      const saved = await evalApi.settingsSet({
        style,
        targetChars: Number(target),
        extraForbidden: extra.split('\n').map((w) => w.trim()).filter(Boolean),
      });
      onSaved(saved);
    } catch (e) {
      onError(errorText(e));
    }
  };
  return (
    <section className="diag-section ev-settings" aria-label={part === 'style' ? '평어 설정: 문체와 금지 표현' : '평어 설정: 목표 글자 수'}>
      {part === 'style' ? (
        <>
          <h3>문장 끝 문체</h3>
          <div className="es-seg-wrap">
            <Seg<EvalStyle>
              label="문체"
              value={style}
              onPick={setStyle}
              options={(Object.keys(STYLE_LABEL) as EvalStyle[]).map((s) => ({ value: s, label: STYLE_LABEL[s] }))}
            />
          </div>
          <label className="rec-field">
            금지 표현 더하기 (한 줄에 낱말 하나)
            <textarea className="rec-textarea" rows={4} value={extra} onChange={(e) => setExtra(e.target.value)} placeholder="예: 학교 기재요령에서 금지하는 낱말" />
          </label>
          <p className="diag-hint">
            기본 금지 표현(서열·사교육·가족 직업과 경제·낙인)은 시작용 목록입니다. 학교 기재요령에 맞춰 위에서 낱말을 더하세요. 문체는 다음에 초안을 만들 때부터 적용됩니다.
          </p>
        </>
      ) : (
        <>
          <label className="rec-field">
            교과별 목표 글자 수 ({LEN_MIN}~{LEN_MAX}, 세 교과를 합친 통합본은 약 3배)
            <input type="number" min={LEN_MIN} max={LEN_MAX} step={10} value={target} onChange={(e) => setTarget(e.target.value)} />
          </label>
          <p className="diag-hint">행동특성 및 종합의견의 목표 글자 수는 학생 화면의 「초안 만들기」 단계에서 정합니다. 글자 수는 다음에 초안을 만들 때부터 적용됩니다.</p>
        </>
      )}
      <div className="es-row">
        <button type="button" className="es-btn es-btn-primary" onClick={run}>
          설정 저장
        </button>
        <button type="button" className="es-btn" onClick={onClose}>
          닫기
        </button>
      </div>
    </section>
  );
}
