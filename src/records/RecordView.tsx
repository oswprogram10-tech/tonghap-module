import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { errorText, recordsApi, type Observation, type Student } from './api';
import ModelBanner from '../stt/ModelBanner';
import BackupReminder from '../backup/BackupReminder';
import ObservationForm from './ObservationForm';
import RecordSearchBar, { EMPTY_FILTERS, MatchList, filtersToQuery, isEmptyQuery, type SearchFilters } from './RecordSearch';
import { RecordsGate, type RecordsCtx } from './useRecords';
import { formatDateTime, shorten, topicTitle } from './util';

const PAGE = 20;

export default function RecordView({ onGoBackup }: { onGoBackup: () => void }) {
  return <RecordsGate>{(ctx) => <RecordPanel ctx={ctx} onGoBackup={onGoBackup} />}</RecordsGate>;
}

function RecordPanel({ ctx, onGoBackup }: { ctx: RecordsCtx; onGoBackup: () => void }) {
  const { cls, classes } = ctx;
  const [students, setStudents] = useState<Student[]>([]);
  const [recent, setRecent] = useState<Observation[]>([]);
  const [editing, setEditing] = useState<Observation | null>(null);
  const [editTopic, setEditTopic] = useState<string | null>(null);
  const [filters, setFilters] = useState<SearchFilters>(EMPTY_FILTERS);
  const [count, setCount] = useState<number | null>(null);
  const [more, setMore] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const toastTimer = useRef<number | null>(null);

  const query = useMemo(() => filtersToQuery(cls.id, filters), [cls.id, filters]);
  const searching = !isEmptyQuery(query);

  useEffect(() => {
    setFilters(EMPTY_FILTERS);
  }, [cls.id]);

  const reload = useCallback(async () => {
    try {
      const [st, obs, total] = await Promise.all([
        recordsApi.studentList(cls.id, false),
        recordsApi.observationList({ ...query, limit: PAGE }),
        isEmptyQuery(query) ? Promise.resolve(null) : recordsApi.observationCount(query),
      ]);
      setStudents(st);
      setRecent(obs);
      setCount(total);
      setMore(obs.length === PAGE);
      return obs;
    } catch (e) {
      setError(errorText(e));
      return [];
    }
  }, [cls.id, query]);
  useEffect(() => {
    void reload();
  }, [reload]);
  useEffect(() => () => { if (toastTimer.current !== null) window.clearTimeout(toastTimer.current); }, []);

  const loadMore = async () => {
    const last = recent[recent.length - 1];
    if (!last) return;
    try {
      const list = await recordsApi.observationList({ ...query, limit: PAGE, before: last.observedAt });
      setRecent((cur) => [...cur, ...list]);
      setMore(list.length === PAGE);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const flash = (message: string) => {
    setToast(message);
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3000);
  };

  const openEdit = (o: Observation) => {
    setEditing(o);
    setEditTopic(o.topicBookId ?? null);
  };

  return (
    <main className="rec">
      <BackupReminder onGoBackup={onGoBackup} />
      <ModelBanner />
      {error && <p className="diag-error">{error}</p>}
      <section className="diag-section">
        <div className="rec-row rec-wrap">
          <h2 className="rec-grow">관찰 기록</h2>
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
        </div>
        <ObservationForm
          key={cls.id}
          cls={cls}
          students={students}
          topicId={ctx.topicId}
          onTopicChange={ctx.selectTopic}
          onSaved={(message) => {
            flash(message);
            void reload();
          }}
        />
        {toast && (
          <div className="rec-toast" role="status">
            {toast}
          </div>
        )}
      </section>

      <section className="diag-section" aria-label="기록 찾기">
        <h2>기록 찾기</h2>
        <RecordSearchBar filters={filters} onChange={setFilters} students={students} count={count} />
        {recent.length === 0 && <p className="diag-hint">{searching ? '조건에 맞는 기록이 없습니다.' : '아직 기록이 없습니다.'}</p>}
        <div className="rec-recent">
          {recent.map((o) => (
            <button key={o.id} className="rec-card" onClick={() => openEdit(o)}>
              <span className="rec-card-head">
                <strong>{formatDateTime(o.observedAt)}</strong>
                <span>{o.students.map((s) => s.name).join(', ')}</span>
              </span>
              <span className="rec-card-meta">
                {o.topicBookId && <span className="rec-tag">{topicTitle(o.topicBookId)}</span>}
                {o.unitSessionTitle && <span className="rec-tag rec-tag-session">차시: {o.unitSessionTitle}</span>}
                {o.needsGuidance && <span className="rec-tag rec-tag-guidance">지도가 필요한 모습</span>}
                {o.tags.map((t) => (
                  <span key={t} className="rec-tag rec-tag-activity">
                    {t}
                  </span>
                ))}
                {o.attachments.length > 0 && <span className="rec-tag">첨부 {o.attachments.length}</span>}
              </span>
              <span className="rec-card-note">{shorten(o.note, 60) || '(메모 없음)'}</span>
              <MatchList matches={o.matches} text={query.text} />
            </button>
          ))}
        </div>
        {more && (
          <button className="diag-secondary" onClick={loadMore}>
            더 보기
          </button>
        )}
      </section>

      {editing && (
        <div className="rec-modal" role="dialog" aria-modal="true" aria-label="기록 수정">
          <div className="rec-modal-body">
            <h2>기록 수정</h2>
            <ObservationForm
              key={editing.id + editing.updatedAt}
              cls={cls}
              students={students}
              topicId={editTopic}
              onTopicChange={setEditTopic}
              editing={editing}
              onSaved={(message, keepOpen) => {
                flash(message);
                void reload().then((list) => {
                  if (keepOpen) {
                    const fresh = list.find((x) => x.id === editing.id);
                    if (fresh) setEditing(fresh);
                  } else {
                    setEditing(null);
                  }
                });
              }}
              onDeleted={() => {
                setEditing(null);
                flash('휴지통으로 보냈습니다. 포트폴리오에서 복원할 수 있습니다.');
                void reload();
              }}
              onCancel={() => setEditing(null)}
            />
          </div>
        </div>
      )}
    </main>
  );
}
