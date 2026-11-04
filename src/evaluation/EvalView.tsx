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
import CombinedPanel from './CombinedPanel';
import StudentDraft from './StudentDraft';

const STATUS_LABEL = { none: '없음', draft: '초안', confirmed: '확정' } as const;
const COMBINED_LABEL = { none: '없음', draft: '초안', confirmed: '확정' } as const;

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
  const [subject, setSubject] = useState<SubjectCode>('슬');
  const [semester, setSemester] = useState<1 | 2 | 0>(currentSemester());
  const initial = semesterPeriod(cls.schoolYear, currentSemester());
  const [from, setFrom] = useState(initial.from);
  const [to, setTo] = useState(initial.to);
  const [rows, setRows] = useState<StudentRow[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [settings, setSettings] = useState<EvalSettings | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [managing, setManaging] = useState(false);
  const [llm, setLlm] = useState<LlmStatus | null>(null);
  const [choice, setChoice] = useState<GeneratorChoice | null>(null);
  const [batch, setBatch] = useState<Batch | null>(null);
  const [rev, setRev] = useState(0);
  const [combinedRows, setCombinedRows] = useState<Record<string, CombinedRow>>({});
  const [combinedTick, setCombinedTick] = useState(0);
  const [exportFormat, setExportFormat] = useState<ExportFormat>('combined');
  const stopBatch = useRef(false);
  const batchRunning = useRef(false);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const periodOk = from !== '' && to !== '' && from <= to;

  const reload = useCallback(async () => {
    if (!periodOk) return;
    try {
      setRows(await evalApi.status(cls.id, subject, from, to));
      setCombinedRows(await evalApi.combinedStatus(cls.id, from, to));
      setCombinedTick((n) => n + 1);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [cls.id, subject, from, to, periodOk]);
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
    setFrom(p.from);
    setTo(p.to);
  };

  const exportAll = async () => {
    setInfo(null);
    setError(null);
    try {
      const path = await saveFile({
        title: '학급 평어 내보내기',
        defaultPath: `평어-${cls.schoolYear}-${cls.name}.txt`,
        filters: [{ name: '텍스트 파일', extensions: ['txt'] }],
      });
      if (!path) return;
      const n = await evalApi.exportTxt(cls.id, from, to, path, exportFormat);
      setInfo(`확정된 ${exportFormat === 'combined' ? '통합본' : '교과별 평어'} ${n}건을 저장했습니다: ${path}`);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const batchTargets = useMemo(() => (rows ?? []).filter((r) => r.status === 'none' && r.evidenceCount >= MIN_EVIDENCE), [rows]);

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

  const confirmedCount = useMemo(() => (rows ?? []).filter((r) => r.status === 'confirmed').length, [rows]);
  const student = rows?.find((r) => r.studentId === selected) ?? null;

  return (
    <main className="rec">
      <div className="rec-row rec-wrap">
        <h2 className="rec-grow">평어 (생활기록부 초안)</h2>
        {classes.length > 1 && (
          <label className="rec-field">
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
        <button className="diag-secondary" onClick={() => setManaging((m) => !m)}>
          학급 관리
        </button>
      </div>
      {managing && (
        <ClassManager
          classes={classes}
          onChanged={async () => {
            await ctx.reloadClasses();
            await reload();
          }}
          onClose={() => setManaging(false)}
        />
      )}

      <p className="ev-notice" role="note">
        초안입니다. 교사의 관찰과 판단으로 고쳐 주세요. 모든 문장은 기록(근거)을 바탕으로 하며, 확정해야 복사·내보내기를 할 수 있습니다.
      </p>
      {error && <p className="diag-error">{error}</p>}
      {info && (
        <p className="rec-toast" role="status">
          {info}
        </p>
      )}

      <div className="rec-row rec-wrap ev-controls">
        <div className="rec-chips" role="group" aria-label="교과 선택">
          {SUBJECT_LIST.map((s) => (
            <button
              key={s.code}
              className={subject === s.code ? 'rec-chip on' : 'rec-chip'}
              aria-pressed={subject === s.code}
              disabled={running}
              onClick={() => {
                setSubject(s.code);
              }}
            >
              {s.name}
            </button>
          ))}
        </div>
        <div className="rec-chips" role="group" aria-label="학기 선택">
          {([1, 2] as const).map((s) => (
            <button key={s} className={semester === s ? 'rec-chip on' : 'rec-chip'} aria-pressed={semester === s} disabled={running} onClick={() => pickSemester(s)}>
              {s}학기
            </button>
          ))}
        </div>
        <label className="rec-field">
          시작일
          <input
            type="date"
            disabled={running}
            value={from}
            onChange={(e) => {
              setSemester(0);
              setFrom(e.target.value);
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
              setSemester(0);
              setTo(e.target.value);
            }}
          />
        </label>
        <button className="diag-secondary" onClick={() => setShowSettings((v) => !v)} aria-expanded={showSettings}>
          문체·금지 표현 설정
        </button>
        <div className="rec-chips" role="group" aria-label="내보내기 형식">
          <button className={exportFormat === 'combined' ? 'rec-chip on' : 'rec-chip'} aria-pressed={exportFormat === 'combined'} onClick={() => setExportFormat('combined')}>
            통합본(NEIS용)
          </button>
          <button className={exportFormat === 'bySubject' ? 'rec-chip on' : 'rec-chip'} aria-pressed={exportFormat === 'bySubject'} onClick={() => setExportFormat('bySubject')}>
            교과별
          </button>
        </div>
        <button className="diag-secondary" onClick={exportAll} disabled={!periodOk}>
          학급 확정본 내보내기 (.txt)
        </button>
      </div>
      <div className="rec-row rec-wrap ev-controls" aria-label="초안 만드는 방식">
        <div className="rec-chips" role="group" aria-label="초안 만드는 방식">
          <button className={generator === 'template' ? 'rec-chip on' : 'rec-chip'} aria-pressed={generator === 'template'} disabled={running} onClick={() => setChoice('template')}>
            템플릿
          </button>
          <button
            className={generator === 'llm' ? 'rec-chip on' : 'rec-chip'}
            aria-pressed={generator === 'llm'}
            disabled={running || !llmReady}
            title={llmReady ? '' : '기기 점검 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요'}
            onClick={() => setChoice('llm')}
          >
            언어 모델
          </button>
        </div>
        <span className="diag-hint ev-gen-hint">
          {llmReady
            ? generator === 'llm'
              ? '이 PC 안의 언어 모델로 만듭니다. 학생 1명에 30초~1분쯤 걸려요.'
              : '문장 틀로 바로 만듭니다.'
            : '언어 모델은 「기기 점검」 탭에서 평어 초안 모델을 준비하면 쓸 수 있어요. 지금은 템플릿만 쓸 수 있습니다.'}
        </span>
        <button className="diag-secondary" onClick={runBatch} disabled={running || !periodOk || batchTargets.length === 0}>
          학급 초안 한꺼번에 만들기 ({batchTargets.length}명)
        </button>
      </div>
      {batch && <BatchPanel batch={batch} onCancel={cancelBatch} onClose={() => setBatch(null)} />}
      {!periodOk && <p className="diag-error">기간을 올바르게 정해 주세요(시작일이 끝일보다 늦을 수 없습니다).</p>}

      {showSettings && settings && <SettingsPanel settings={settings} onSaved={(s) => { setSettings(s); setInfo('설정을 저장했습니다'); }} onError={(m) => setError(m)} />}

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
              <span className={`ev-status ev-status-${r.status}`} title="이 교과 평어">
                {STATUS_LABEL[r.status]}
              </span>
              <span className={`ev-status ev-status-${combinedRows[r.studentId]?.status ?? 'none'}`} title="통합본">
                통합 {COMBINED_LABEL[combinedRows[r.studentId]?.status ?? 'none']}
                {combinedRows[r.studentId]?.stale ? '·바뀜' : ''}
              </span>
            </button>
          ))}
          {rows && rows.length > 0 && <p className="diag-hint">확정 {confirmedCount}/{rows.length}명 (이 교과)</p>}
        </aside>
        <div className="rec-main">
          {student && settings && periodOk ? (
            <>
            <StudentDraft
              key={`${student.studentId}|${subject}|${from}|${to}|${rev}`}
              studentId={student.studentId}
              subject={subject}
              from={from}
              to={to}
              settings={settings}
              generator={generator}
              locked={running}
              onChanged={reload}
            />
            <CombinedPanel key={`c|${student.studentId}|${from}|${to}|${rev}`} studentId={student.studentId} from={from} to={to} refresh={combinedTick} onChanged={reload} />
            </>
          ) : (
            <p className="diag-note">왼쪽에서 학생을 고르면 이 교과의 근거 기록과 평어 초안을 볼 수 있어요.</p>
          )}
        </div>
      </div>
    </main>
  );
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

function SettingsPanel({ settings, onSaved, onError }: { settings: EvalSettings; onSaved: (s: EvalSettings) => void; onError: (m: string) => void }) {
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
    <section className="diag-section ev-settings" aria-label="평어 설정">
      <h3>문장 끝 문체</h3>
      <div className="rec-chips" role="group" aria-label="문체">
        {(Object.keys(STYLE_LABEL) as EvalStyle[]).map((s) => (
          <button key={s} className={style === s ? 'rec-chip on' : 'rec-chip'} aria-pressed={style === s} onClick={() => setStyle(s)}>
            {STYLE_LABEL[s]}
          </button>
        ))}
      </div>
      <label className="rec-field">
        교과별 목표 글자 수 ({LEN_MIN}~{LEN_MAX}, 세 교과를 합친 통합본은 약 3배)
        <input type="number" min={LEN_MIN} max={LEN_MAX} step={10} value={target} onChange={(e) => setTarget(e.target.value)} />
      </label>
      <label className="rec-field">
        금지 표현 더하기 (한 줄에 낱말 하나)
        <textarea className="rec-textarea" rows={4} value={extra} onChange={(e) => setExtra(e.target.value)} placeholder="예: 학교 기재요령에서 금지하는 낱말" />
      </label>
      <p className="diag-hint">
        기본 금지 표현(서열·사교육·가족 직업과 경제·낙인)은 시작용 목록입니다. 학교 기재요령에 맞춰 위에서 낱말을 더하세요. 문체와 글자 수는 다음에 초안을 만들 때부터 적용됩니다.
      </p>
      <button className="diag-primary" onClick={run}>
        설정 저장
      </button>
    </section>
  );
}
