import { useCallback, useEffect, useRef, useState } from 'react';
import { errorText, recordsApi, type Observation, type Student } from './api';
import ObservationForm from './ObservationForm';
import { RecordsGate, type RecordsCtx } from './useRecords';
import { formatDateTime, shorten, topicTitle } from './util';

export default function RecordView() {
  return <RecordsGate>{(ctx) => <RecordPanel ctx={ctx} />}</RecordsGate>;
}

function RecordPanel({ ctx }: { ctx: RecordsCtx }) {
  const { cls, classes } = ctx;
  const [students, setStudents] = useState<Student[]>([]);
  const [recent, setRecent] = useState<Observation[]>([]);
  const [editing, setEditing] = useState<Observation | null>(null);
  const [editTopic, setEditTopic] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const toastTimer = useRef<number | null>(null);

  const reload = useCallback(async () => {
    try {
      const [st, obs] = await Promise.all([
        recordsApi.studentList(cls.id, false),
        recordsApi.observationList({ classId: cls.id, limit: 20 }),
      ]);
      setStudents(st);
      setRecent(obs);
      return obs;
    } catch (e) {
      setError(errorText(e));
      return [];
    }
  }, [cls.id]);
  useEffect(() => {
    void reload();
  }, [reload]);
  useEffect(() => () => { if (toastTimer.current !== null) window.clearTimeout(toastTimer.current); }, []);

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

      <section className="diag-section" aria-label="최근 기록">
        <h2>최근 기록</h2>
        {recent.length === 0 && <p className="diag-hint">아직 기록이 없습니다.</p>}
        <div className="rec-recent">
          {recent.map((o) => (
            <button key={o.id} className="rec-card" onClick={() => openEdit(o)}>
              <span className="rec-card-head">
                <strong>{formatDateTime(o.observedAt)}</strong>
                <span>{o.students.map((s) => s.name).join(', ')}</span>
              </span>
              <span className="rec-card-meta">
                {o.topicBookId && <span className="rec-tag">{topicTitle(o.topicBookId)}</span>}
                {o.tags.map((t) => (
                  <span key={t} className="rec-tag rec-tag-activity">
                    {t}
                  </span>
                ))}
                {o.attachments.length > 0 && <span className="rec-tag">첨부 {o.attachments.length}</span>}
              </span>
              <span className="rec-card-note">{shorten(o.note, 60) || '(메모 없음)'}</span>
            </button>
          ))}
        </div>
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
