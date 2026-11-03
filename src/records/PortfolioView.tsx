import { useCallback, useEffect, useMemo, useState } from 'react';
import { SEMESTERS, curriculum, topicsOf } from '../curriculum';
import { errorText, recordsApi, type ClassOverview, type Observation, type Student, type StudentPortfolio } from './api';
import { HabitSummaryPanel, HabitWeekPanel } from '../board/HabitWeek';
import AttachmentView from './Attachments';
import ClassManager from './ClassManager';
import ObservationForm from './ObservationForm';
import { RecordsGate, type RecordsCtx } from './useRecords';
import { formatDate, formatDateTime, topicTitle, topicsOfGrade } from './util';

const PAGE = 20;

export default function PortfolioView() {
  return <RecordsGate>{(ctx) => <PortfolioPanel ctx={ctx} />}</RecordsGate>;
}

function PortfolioPanel({ ctx }: { ctx: RecordsCtx }) {
  const { cls, classes } = ctx;
  const [students, setStudents] = useState<Student[]>([]);
  const [overview, setOverview] = useState<ClassOverview | null>(null);
  const [selected, setSelected] = useState<string | 'class'>('class');
  const [managing, setManaging] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const [st, ov] = await Promise.all([recordsApi.studentList(cls.id, false), recordsApi.portfolioClass(cls.id)]);
      setStudents(st);
      setOverview(ov);
    } catch (e) {
      setError(errorText(e));
    }
  }, [cls.id]);
  useEffect(() => {
    setSelected('class');
    void reload();
  }, [reload]);

  const totals = useMemo(() => new Map((overview?.students ?? []).map((r) => [r.student.id, r.total])), [overview]);
  const student = students.find((s) => s.id === selected);

  return (
    <main className="rec">
      {error && <p className="diag-error">{error}</p>}
      <div className="rec-row rec-wrap">
        <h2 className="rec-grow">포트폴리오</h2>
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

      <div className="rec-portfolio">
        <aside className="rec-sidebar" aria-label="학생 목록">
          <button className={selected === 'class' ? 'rec-side active' : 'rec-side'} onClick={() => setSelected('class')}>
            학급 현황
          </button>
          {students.map((s) => (
            <button key={s.id} className={selected === s.id ? 'rec-side active' : 'rec-side'} onClick={() => setSelected(s.id)}>
              <span className="rec-chip-num">{s.number}</span>
              <span className="rec-grow">{s.name}</span>
              <span className="rec-count">{totals.get(s.id) ?? 0}</span>
            </button>
          ))}
        </aside>
        <div className="rec-main">
          {selected === 'class' || !student ? (
            <>
              <ClassTable overview={overview} grade={cls.grade} onPick={(id) => setSelected(id)} />
              <HabitWeekPanel classId={cls.id} />
            </>
          ) : (
            <StudentPanel key={student.id} cls={cls} student={student} students={students} onChanged={reload} />
          )}
        </div>
      </div>
    </main>
  );
}

function ClassTable({ overview, grade, onPick }: { overview: ClassOverview | null; grade: number; onPick: (id: string) => void }) {
  if (!overview) return <p className="diag-note">불러오는 중…</p>;
  const sems = SEMESTERS.filter((s) => s.grade === grade).map((s) => ({ ...s, topics: topicsOf(s.grade, s.semester) }));
  return (
    <section className="diag-section">
      <h2>학급 현황</h2>
      <p className="diag-hint">빨간 0은 아직 기록이 없는 칸입니다. 이름을 누르면 그 학생의 포트폴리오로 갑니다.</p>
      <div className="rec-table-wrap">
        <table className="rec-table">
          <thead>
            <tr>
              <th rowSpan={2}>학생</th>
              {sems.map((s) => (
                <th key={s.semester} colSpan={s.topics.length}>
                  {s.semester}학기
                </th>
              ))}
              <th rowSpan={2}>합계</th>
              <th rowSpan={2}>마지막 기록</th>
            </tr>
            <tr>
              {sems.flatMap((s) => s.topics).map((t) => (
                <th key={t.id} title={t.title}>
                  {t.title}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {overview.students.map((r) => (
              <tr key={r.student.id}>
                <th>
                  <button className="rec-link" onClick={() => onPick(r.student.id)}>
                    {r.student.number} {r.student.name}
                  </button>
                </th>
                {sems.flatMap((s) => s.topics).map((t) => {
                  const n = r.byTopicBook[t.id] ?? 0;
                  return (
                    <td key={t.id} className={n === 0 ? 'rec-zero' : ''}>
                      {n}
                    </td>
                  );
                })}
                <td className={r.total === 0 ? 'rec-zero' : 'rec-total'}>{r.total}</td>
                <td>{formatDate(r.lastObservedAt)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function StudentPanel({
  cls,
  student,
  students,
  onChanged,
}: {
  cls: { id: string; grade: number; schoolYear: number; name: string; createdAt: string };
  student: Student;
  students: Student[];
  onChanged: () => Promise<void>;
}) {
  const [portfolio, setPortfolio] = useState<StudentPortfolio | null>(null);
  const [items, setItems] = useState<Observation[]>([]);
  const [more, setMore] = useState(false);
  const [topicFilter, setTopicFilter] = useState('');
  const [codeFilter, setCodeFilter] = useState<string | null>(null);
  const [trash, setTrash] = useState(false);
  const [editing, setEditing] = useState<Observation | null>(null);
  const [editTopic, setEditTopic] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const query = useCallback(
    (before?: string) => ({
      classId: cls.id,
      studentId: student.id,
      topicBookId: topicFilter || undefined,
      standardCode: codeFilter ?? undefined,
      deletedOnly: trash,
      limit: PAGE,
      before,
    }),
    [cls.id, student.id, topicFilter, codeFilter, trash],
  );

  const loadFirst = useCallback(async () => {
    try {
      const list = await recordsApi.observationList(query());
      setItems(list);
      setMore(list.length === PAGE);
    } catch (e) {
      setError(errorText(e));
    }
  }, [query]);
  const loadPortfolio = useCallback(async () => {
    try {
      setPortfolio(await recordsApi.portfolioStudent(student.id));
    } catch (e) {
      setError(errorText(e));
    }
  }, [student.id]);

  useEffect(() => {
    void loadFirst();
  }, [loadFirst]);
  useEffect(() => {
    void loadPortfolio();
  }, [loadPortfolio]);

  const refresh = async () => {
    await Promise.all([loadFirst(), loadPortfolio(), onChanged()]);
  };

  const loadMore = async () => {
    const last = items[items.length - 1];
    if (!last) return;
    try {
      const list = await recordsApi.observationList(query(last.observedAt));
      setItems((cur) => [...cur, ...list]);
      setMore(list.length === PAGE);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const act = (fn: () => Promise<void>) => () => fn().then(refresh).catch((e) => setError(errorText(e)));

  const std = new Map((portfolio?.byStandard ?? []).map((x) => [x.standardCode, x.count]));
  const topicCount = new Map((portfolio?.byTopicBook ?? []).map((x) => [x.topicBookId, x.count]));
  const gradeTopics = topicsOfGrade(cls.grade);
  const topicsAll = curriculum.topicBooks;
  const gradeKey = new Set(gradeTopics.map((t) => `${t.area}-${t.standardIndex}`));

  const areas = curriculum.areas;
  const gridRows = areas.flatMap((a) =>
    [1, 2, 3, 4].map((idx) => ({
      area: a.number,
      index: idx,
      cells: curriculum.subjects.map((s) => curriculum.standards.find((x) => x.subject === s.code && x.area === a.number && x.index === idx)),
    })),
  );

  return (
    <div className="rec-student">
      {error && <p className="diag-error">{error}</p>}
      <section className="diag-section">
        <h2>
          {student.number}번 {student.name}
        </h2>
        {portfolio && (
          <>
            <div className="rec-summary">
              <div>
                <span className="rec-big">{portfolio.observationCount}</span>
                <span>기록</span>
              </div>
              <div>
                <span>첫 기록</span>
                <strong>{formatDate(portfolio.firstObservedAt)}</strong>
              </div>
              <div>
                <span>마지막 기록</span>
                <strong>{formatDate(portfolio.lastObservedAt)}</strong>
              </div>
            </div>
            <div className="rec-chips">
              {portfolio.byTag.length === 0 && <span className="diag-hint">태그가 달린 기록이 없습니다.</span>}
              {portfolio.byTag.map((t) => (
                <span key={t.tag} className="rec-chip-static">
                  {t.tag} {t.count}
                </span>
              ))}
            </div>
          </>
        )}
      </section>

      <HabitSummaryPanel studentId={student.id} />

      <section className="diag-section">
        <h3>성취기준별 근거</h3>
        <p className="diag-hint">진한 행은 현재 학년 주제책의 성취기준입니다. 칸을 누르면 그 성취기준의 기록만 봅니다.</p>
        <table className="rec-table rec-grid">
          <thead>
            <tr>
              <th>영역</th>
              {curriculum.subjects.map((s) => (
                <th key={s.code}>{s.name}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {gridRows.map((row) => (
              <tr key={`${row.area}-${row.index}`} className={gradeKey.has(`${row.area}-${row.index}`) ? 'rec-grade-row' : ''}>
                <th>
                  {row.area}-{row.index}
                </th>
                {row.cells.map((c, i) => {
                  if (!c) return <td key={i}>—</td>;
                  const n = std.get(c.code) ?? 0;
                  return (
                    <td key={c.code}>
                      <button
                        className={codeFilter === c.code ? 'rec-cell on' : n > 0 ? 'rec-cell has' : 'rec-cell'}
                        aria-pressed={codeFilter === c.code}
                        title={c.statement ?? c.code}
                        onClick={() => setCodeFilter(codeFilter === c.code ? null : c.code)}
                      >
                        <code>{c.code}</code> <strong>{n}</strong>
                      </button>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="diag-section">
        <div className="rec-row rec-wrap">
          <h3 className="rec-grow">{trash ? '휴지통' : '타임라인'}</h3>
          <label className="rec-field">
            주제책
            <select value={topicFilter} onChange={(e) => setTopicFilter(e.target.value)}>
              <option value="">전체</option>
              {topicsAll.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.grade}학년 {t.semester}학기 · {t.title} ({topicCount.get(t.id) ?? 0})
                </option>
              ))}
            </select>
          </label>
          {codeFilter && (
            <button className="diag-secondary" onClick={() => setCodeFilter(null)}>
              {codeFilter} 해제
            </button>
          )}
          <button className="diag-secondary" onClick={() => setTrash((t) => !t)}>
            {trash ? '타임라인 보기' : '휴지통 보기'}
          </button>
        </div>
        {items.length === 0 && <p className="diag-hint">{trash ? '휴지통이 비어 있습니다.' : '조건에 맞는 기록이 없습니다.'}</p>}
        {items.map((o) => (
          <article key={o.id} className="rec-entry">
            <div className="rec-card-head">
              <strong>{formatDateTime(o.observedAt)}</strong>
              {o.topicBookId && <span className="rec-tag">{topicTitle(o.topicBookId)}</span>}
              {o.unitSessionTitle && <span className="rec-tag rec-tag-session">차시: {o.unitSessionTitle}</span>}
              {o.students.length > 1 && <span className="rec-tag">함께: {o.students.filter((s) => s.id !== student.id).map((s) => s.name).join(', ')}</span>}
            </div>
            <div className="rec-card-meta">
              {o.standardCodes.map((c) => (
                <code key={c} className="rec-code">
                  {c}
                </code>
              ))}
              {o.tags.map((t) => (
                <span key={t} className="rec-tag rec-tag-activity">
                  {t}
                </span>
              ))}
            </div>
            {o.note && <p className="rec-note">{o.note}</p>}
            <AttachmentView attachments={o.attachments} />
            <div className="rec-row rec-wrap">
              {trash ? (
                <>
                  <button className="diag-secondary" onClick={act(() => recordsApi.observationRestore(o.id))}>
                    복원
                  </button>
                  <button
                    className="diag-secondary"
                    onClick={() => {
                      if (window.confirm('완전히 삭제할까요? 첨부 파일도 함께 지워지며 되돌릴 수 없습니다.')) void act(() => recordsApi.observationPurge(o.id))();
                    }}
                  >
                    완전 삭제
                  </button>
                </>
              ) : (
                <button
                  className="diag-secondary"
                  onClick={() => {
                    setEditing(o);
                    setEditTopic(o.topicBookId ?? null);
                  }}
                >
                  수정
                </button>
              )}
            </div>
          </article>
        ))}
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
              cls={{ ...cls }}
              students={students}
              topicId={editTopic}
              onTopicChange={setEditTopic}
              editing={editing}
              onSaved={(_m, keepOpen) => {
                void refresh().then(() => {
                  if (!keepOpen) setEditing(null);
                });
              }}
              onDeleted={() => {
                setEditing(null);
                void refresh();
              }}
              onCancel={() => setEditing(null)}
            />
          </div>
        </div>
      )}
    </div>
  );
}
