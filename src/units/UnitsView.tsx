import { useCallback, useEffect, useState } from 'react';
import { errorText } from '../records/api';
import { RecordsGate, type RecordsCtx } from '../records/useRecords';
import { topicTitle } from '../records/util';
import { unitsApi, type UnitSummary } from './api';
import RequirementBadges from './Badges';
import NewUnit from './NewUnit';
import UnitBoard from './UnitBoard';
import { toSessions, toolName, topicTotalSessions } from './util';

export default function UnitsView() {
  return <RecordsGate what="단원 빌더">{(ctx) => <UnitsPanel ctx={ctx} />}</RecordsGate>;
}

type Screen = { kind: 'list' } | { kind: 'new' } | { kind: 'board'; unitId: string };

function UnitsPanel({ ctx }: { ctx: RecordsCtx }) {
  const { cls, classes } = ctx;
  const [screen, setScreen] = useState<Screen>({ kind: 'list' });
  const [units, setUnits] = useState<UnitSummary[] | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setUnits(await unitsApi.list(cls.id, showArchived));
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [cls.id, showArchived]);
  useEffect(() => {
    void reload();
  }, [reload]);

  // 학급을 바꾸면 목록으로 돌아간다.
  useEffect(() => {
    setScreen({ kind: 'list' });
  }, [cls.id]);

  return (
    <main className="rec units">
      {screen.kind === 'new' ? (
        <NewUnit
          cls={cls}
          initialTopicId={ctx.topicId}
          onCancel={() => setScreen({ kind: 'list' })}
          onCreated={(u) => {
            void reload();
            setScreen({ kind: 'board', unitId: u.id });
          }}
        />
      ) : screen.kind === 'board' ? (
        <UnitBoard
          key={screen.unitId}
          cls={cls}
          unitId={screen.unitId}
          onBack={() => {
            void reload();
            setScreen({ kind: 'list' });
          }}
        />
      ) : (
        <section className="diag-section">
          <div className="rec-row rec-wrap">
            <h2 className="rec-grow">단원</h2>
            {classes.length > 1 && (
              <label className="rec-field">
                학급
                <select value={cls.id} onChange={(e) => ctx.selectClass(e.target.value)}>
                  {classes.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.schoolYear}학년도 {c.grade}학년 {c.name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {classes.length === 1 && (
              <span className="rec-class-label">
                {cls.schoolYear}학년도 {cls.grade}학년 {cls.name}
              </span>
            )}
            <button className="diag-secondary" aria-pressed={showArchived} onClick={() => setShowArchived((v) => !v)}>
              {showArchived ? '보관함 닫기' : '보관함 보기'}
            </button>
            <button className="diag-primary" onClick={() => setScreen({ kind: 'new' })}>
              새 단원
            </button>
          </div>
          {error && <p className="diag-error">{error}</p>}
          {units && units.length === 0 && (
            <p className="diag-hint">{showArchived ? '단원이 없습니다.' : '아직 단원이 없습니다. 「새 단원」으로 시작해 보세요.'}</p>
          )}
          <div className="unit-list">
            {units?.map((u) => (
              <button key={u.id} className={u.archivedAt ? 'unit-card archived' : 'unit-card'} onClick={() => setScreen({ kind: 'board', unitId: u.id })}>
                <span className="unit-card-title">{u.title}</span>
                <span className="unit-card-meta">
                  {u.topicBookId ? <span className="rec-tag">{topicTitle(u.topicBookId)}</span> : <span className="rec-tag">주제책 없음</span>}
                  <span className="rec-tag">{toolName(u.toolId)}</span>
                  {u.archivedAt && <span className="rec-tag">보관됨</span>}
                </span>
                <span className="unit-card-meta">
                  <span>
                    진행 {u.totals.doneSessions}/{u.totals.sessions}차시
                  </span>
                  <span>
                    시수 {toSessions(u.totals.minutes)} / {topicTotalSessions(u.topicBookId)}차시
                  </span>
                </span>
                <RequirementBadges totals={u.totals} />
              </button>
            ))}
          </div>
        </section>
      )}
    </main>
  );
}
