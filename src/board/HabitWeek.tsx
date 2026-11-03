import './board.css';
import { useEffect, useState } from 'react';
import { errorText } from '../records/api';
import { boardApi, dateStr, shortDate, type Habit, type HabitSummary, type HabitWeek } from './api';

/** 학생 × 최근 7일 표. 칠판 모드와 포트폴리오에서 같이 쓴다. `version`이 바뀌면 다시 불러온다. */
export function HabitWeekTable({ habit, version = 0 }: { habit: Habit; version?: number }) {
  const [week, setWeek] = useState<HabitWeek | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    boardApi
      .habitWeek(habit.id, dateStr())
      .then((w) => live && setWeek(w))
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [habit.id, version]);

  if (error) return <p className="diag-error">{error}</p>;
  if (!week) return <p className="diag-note">불러오는 중…</p>;
  return (
    <div className="rec-table-wrap">
      <table className="rec-table bd-week">
        <thead>
          <tr>
            <th>학생</th>
            {week.dates.map((d) => (
              <th key={d}>{shortDate(d)}</th>
            ))}
            <th>7일 중</th>
          </tr>
        </thead>
        <tbody>
          {week.rows.map((r) => (
            <tr key={r.studentId}>
              <th>
                {r.number} {r.name}
              </th>
              {r.marks.map((m, i) => (
                <td key={i} className={m === true ? 'bd-w-yes' : m === false ? 'bd-w-no' : 'bd-w-none'} title={m === null ? '체크한 사람이 없는 날' : m ? '했어요' : '아직'}>
                  {m === true ? '●' : m === false ? '○' : '–'}
                </td>
              ))}
              <td>
                <strong>{r.doneDays}</strong>일
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** 포트폴리오 학급 현황용: 습관을 고르면 주간 표를 보여 준다. */
export function HabitWeekPanel({ classId }: { classId: string }) {
  const [habits, setHabits] = useState<Habit[]>([]);
  const [sel, setSel] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    boardApi
      .habitList(classId, false)
      .then((h) => {
        if (!live) return;
        setHabits(h);
        setSel((cur) => (h.some((x) => x.id === cur) ? cur : (h[0]?.id ?? '')));
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [classId]);
  const habit = habits.find((h) => h.id === sel);
  return (
    <section className="diag-section">
      <div className="rec-row rec-wrap">
        <h2 className="rec-grow">습관 신호등 주간 보기</h2>
        {habits.length > 0 && (
          <label className="rec-field">
            습관
            <select value={sel} onChange={(e) => setSel(e.target.value)}>
              {habits.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.icon} {h.title}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {error && <p className="diag-error">{error}</p>}
      {habits.length === 0 ? <p className="diag-hint">칠판 모드의 「습관 신호등」에서 습관을 만들면 여기에 표시됩니다.</p> : habit && <HabitWeekTable habit={habit} />}
    </section>
  );
}

/** 포트폴리오 학생 화면: 습관별 최근 28일 실천율. */
export function HabitSummaryPanel({ studentId }: { studentId: string }) {
  const [rows, setRows] = useState<HabitSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    boardApi
      .habitSummary(studentId, dateStr(), 28)
      .then((r) => live && setRows(r))
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [studentId]);
  if (error) return <p className="diag-error">{error}</p>;
  if (!rows || rows.length === 0) return null;
  return (
    <section className="diag-section">
      <h3>습관 실천 (최근 28일)</h3>
      <p className="diag-hint">실천한 날 ÷ 그 습관에 체크가 한 번이라도 있었던 날 수</p>
      {rows.map((r) => {
        const pct = r.schoolDays > 0 ? Math.round((r.doneDays / r.schoolDays) * 100) : null;
        return (
          <div key={r.habitId} className="bd-sum-row">
            <span className="bd-sum-title">
              {r.icon} {r.title}
            </span>
            <span className="bd-sum-bar" aria-hidden="true">
              <span style={{ width: `${pct ?? 0}%` }} />
            </span>
            <span className="bd-sum-val">
              {r.doneDays}/{r.schoolDays}일{pct !== null && ` (${pct}%)`}
            </span>
          </div>
        );
      })}
    </section>
  );
}
