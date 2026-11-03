import { useCallback, useEffect, useMemo, useState } from 'react';
import { errorText, type ClassInfo } from '../records/api';
import { standardByCode, topicStandards, topicTitle, topicsOfGrade, formatDate, shorten } from '../records/util';
import { unitsApi, type Session, type UnitDetail } from './api';
import RequirementBadges from './Badges';
import SessionEditor from './SessionEditor';
import { KIND_LABEL, MODE_SHORT, STATUS_LABEL, formatMinutes, toSessions, toolName, topicTotalSessions } from './util';

interface Props {
  cls: ClassInfo;
  unitId: string;
  onBack: () => void;
}

type Editor = { session: Session | null; stageId: string } | null;

export default function UnitBoard({ cls, unitId, onBack }: Props) {
  const [unit, setUnit] = useState<UnitDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<Editor>(null);
  const [title, setTitle] = useState('');
  const [memo, setMemo] = useState('');
  const [topicId, setTopicId] = useState<string | null>(null);
  const [stageDrafts, setStageDrafts] = useState<Record<string, string>>({});
  const [notice, setNotice] = useState<string | null>(null);

  const topics = useMemo(() => topicsOfGrade(cls.grade), [cls.grade]);

  const adopt = useCallback((u: UnitDetail) => {
    setUnit(u);
    setTitle(u.title);
    setMemo(u.memo);
    setTopicId(u.topicBookId);
    setStageDrafts({});
  }, []);

  const reload = useCallback(async () => {
    try {
      adopt(await unitsApi.get(unitId));
    } catch (e) {
      setError(errorText(e));
    }
  }, [unitId, adopt]);
  useEffect(() => {
    void reload();
  }, [reload]);

  if (!unit) {
    return <section className="diag-section">{error ? <p className="diag-error">{error}</p> : <p className="diag-note">불러오는 중…</p>}</section>;
  }

  const act = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const dirty = title !== unit.title || memo !== unit.memo || topicId !== unit.topicBookId;
  const saveHead = () =>
    act(async () => {
      adopt(await unitsApi.update(unit.id, title, memo, topicId));
      setNotice('저장했습니다.');
    });

  const renameStage = (id: string, current: string) => {
    const draft = (stageDrafts[id] ?? current).trim();
    if (draft === current) return;
    void act(async () => {
      await unitsApi.stageRename(id, draft);
      await reload();
    });
  };

  const move = (s: Session, delta: number) => act(async () => adopt(await unitsApi.sessionMove(s.id, s.stageId, s.position + delta)));

  const archived = unit.archivedAt !== null;
  const toggleArchive = () =>
    act(async () => {
      if (archived) await unitsApi.unarchive(unit.id);
      else await unitsApi.archive(unit.id);
      await reload();
    });
  const remove = () => {
    if (!window.confirm('이 단원과 차시를 모두 지울까요? 연결된 관찰 기록은 남고 차시 연결만 끊어집니다.')) return;
    void act(async () => {
      await unitsApi.remove(unit.id);
      onBack();
    });
  };

  const t = unit.totals;
  const topic = topicStandards(unit.topicBookId);
  const topicCodes = topic.map((x) => x.standard.code);
  const usedCodes = t.standards.map((s) => s.code);
  const tableCodes = [...topicCodes, ...usedCodes.filter((c) => !topicCodes.includes(c))];

  return (
    <>
      <section className="diag-section">
        <div className="rec-row rec-wrap">
          <button className="diag-secondary" onClick={onBack}>
            목록으로
          </button>
          <span className="rec-grow" />
          <button className="diag-secondary" onClick={toggleArchive}>
            {archived ? '보관 해제' : '보관'}
          </button>
          <button className="diag-secondary" onClick={remove}>
            단원 삭제
          </button>
        </div>
        {archived && <p className="diag-hint">보관된 단원입니다. 관찰 기록 화면의 차시 선택에는 나오지 않습니다.</p>}
        <div className="rec-row rec-wrap" style={{ marginTop: 12 }}>
          <label className="rec-field rec-grow">
            단원 제목
            <input value={title} maxLength={100} onChange={(e) => setTitle(e.target.value)} />
          </label>
          <label className="rec-field">
            주제책
            <select value={topicId ?? ''} onChange={(e) => setTopicId(e.target.value || null)}>
              <option value="">(주제책 없음)</option>
              {topics.map((tb) => (
                <option key={tb.id} value={tb.id}>
                  {tb.semester}학기 {tb.order}. {tb.title}
                </option>
              ))}
            </select>
          </label>
          <span className="rec-class-label">도구: {toolName(unit.toolId)}</span>
        </div>
        <label className="rec-field" style={{ marginTop: 8 }}>
          메모
          <textarea className="rec-textarea" rows={2} value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        <div className="rec-row rec-wrap" style={{ marginTop: 8 }}>
          <button className="diag-primary" disabled={!dirty || title.trim() === ''} onClick={saveHead}>
            제목·주제책·메모 저장
          </button>
          {notice && !dirty && (
            <span className="diag-hint" role="status">
              {notice}
            </span>
          )}
        </div>
        {error && <p className="diag-error">{error}</p>}
      </section>

      <section className="diag-section unit-summary" aria-label="집계">
        <div className="rec-row rec-wrap">
          <strong className="unit-total">
            시수 {toSessions(t.minutes)} / {topicTotalSessions(unit.topicBookId)}차시
          </strong>
          <span>
            진행 {t.doneSessions}/{t.sessions}차시 (완료 {toSessions(t.doneMinutes)}차시)
          </span>
          <RequirementBadges totals={t} />
        </div>
        <h3 className="rec-h">성취기준 반영 {unit.topicBookId ? `· ${topicTitle(unit.topicBookId)}` : ''}</h3>
        {tableCodes.length === 0 ? (
          <p className="diag-hint">주제책을 고르거나 차시에 성취기준을 넣으면 여기에 모입니다.</p>
        ) : (
          <table className="unit-std-table">
            <thead>
              <tr>
                <th>성취기준</th>
                <th>반복형</th>
                <th>집중형</th>
                <th>발현형</th>
              </tr>
            </thead>
            <tbody>
              {tableCodes.map((code) => {
                const row = t.standards.find((s) => s.code === code);
                const info = standardByCode(code);
                const zero = !row || row.repeat + row.focus + row.emergent === 0;
                return (
                  <tr key={code} className={zero ? 'zero' : ''}>
                    <td>
                      {info && <span className={`subject-chip subject-${info.subject.id}`}>{info.subject.name}</span>} <code>{code}</code>{' '}
                      <span className="rec-chip-text">{shorten(info?.standard.statement, 28)}</span>
                    </td>
                    <td>{row?.repeat ?? 0}</td>
                    <td>{row?.focus ?? 0}</td>
                    <td>{row?.emergent ?? 0}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      {unit.stages.map((stage) => (
        <section key={stage.id} className="diag-section unit-stage" aria-label={`단계 ${stage.position + 1}`}>
          <div className="rec-row rec-wrap">
            <span className="unit-stage-no">{stage.position + 1}</span>
            <input
              className="rec-grow unit-stage-title"
              aria-label={`단계 ${stage.position + 1} 제목`}
              value={stageDrafts[stage.id] ?? stage.title}
              maxLength={100}
              onChange={(e) => setStageDrafts((d) => ({ ...d, [stage.id]: e.target.value }))}
              onBlur={() => renameStage(stage.id, stage.title)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
              }}
            />
          </div>
          {stage.sessions.length === 0 && <p className="diag-hint">아직 차시가 없습니다.</p>}
          <ol className="unit-sessions">
            {stage.sessions.map((s, i) => (
              <li key={s.id} className={`unit-session status-${s.status}`}>
                <button className="unit-session-main" onClick={() => setEditor({ session: s, stageId: stage.id })} aria-label={`차시 ${s.title} 편집`}>
                  <span className="unit-session-head">
                    <span className="unit-session-no">{i + 1}</span>
                    <strong>{s.title}</strong>
                    <span className={`unit-kind kind-${s.kind}`}>{KIND_LABEL[s.kind]}</span>
                    <span className={`unit-status st-${s.status}`}>{STATUS_LABEL[s.status]}</span>
                  </span>
                  <span className="unit-session-meta">
                    <span>{formatMinutes(s.minutes)}</span>
                    <span>{s.scheduledOn ? formatDate(s.scheduledOn) : '날짜 없음'}</span>
                    <span>관찰 기록 {s.observationCount}</span>
                    {s.followsSessionId && <span className="unit-follow">↳ 앞 차시에서 이어짐</span>}
                  </span>
                  {s.standards.length > 0 && (
                    <span className="unit-session-meta">
                      {s.standards.map((st) => (
                        <span key={st.code} className="rec-code">
                          {st.code} {MODE_SHORT[st.mode]}
                        </span>
                      ))}
                    </span>
                  )}
                </button>
                <span className="unit-session-actions">
                  <button className="diag-secondary" disabled={i === 0} aria-label={`${s.title} 위로`} onClick={() => move(s, -1)}>
                    위로
                  </button>
                  <button className="diag-secondary" disabled={i === stage.sessions.length - 1} aria-label={`${s.title} 아래로`} onClick={() => move(s, 1)}>
                    아래로
                  </button>
                </span>
              </li>
            ))}
          </ol>
          <button className="diag-secondary" onClick={() => setEditor({ session: null, stageId: stage.id })}>
            차시 추가
          </button>
        </section>
      ))}

      {editor && (
        <SessionEditor
          key={editor.session?.id ?? `new-${editor.stageId}`}
          unit={unit}
          session={editor.session}
          stageId={editor.stageId}
          onClose={() => setEditor(null)}
          onChanged={(u) => adopt(u)}
          onOpenSession={(s) => setEditor({ session: s, stageId: s.stageId })}
        />
      )}
    </>
  );
}
