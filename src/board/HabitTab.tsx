import { useCallback, useEffect, useState } from 'react';
import { errorText, type ClassInfo, type Student } from '../records/api';
import { addDays, boardApi, dateStr, shortDate, type Habit, type HabitDay } from './api';
import { HabitWeekTable } from './HabitWeek';

const MAX_BACK = 7;

export default function HabitTab({ cls, students }: { cls: ClassInfo | null; students: Student[] }) {
  const [habits, setHabits] = useState<Habit[]>([]);
  const [sel, setSel] = useState<string | null>(null);
  const [date, setDate] = useState(dateStr());
  const [day, setDay] = useState<HabitDay | null>(null);
  const [view, setView] = useState<'day' | 'week' | 'manage'>('day');
  const [version, setVersion] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const classId = cls?.id ?? null;
  const loadHabits = useCallback(async () => {
    if (!classId) return setHabits([]);
    try {
      const list = await boardApi.habitList(classId, false);
      setHabits(list);
      setSel((cur) => (list.some((h) => h.id === cur) ? cur : (list[0]?.id ?? null)));
    } catch (e) {
      setError(errorText(e));
    }
  }, [classId]);
  useEffect(() => {
    void loadHabits();
  }, [loadHabits, students]);

  useEffect(() => {
    if (!sel) return setDay(null);
    let live = true;
    boardApi
      .habitDay(sel, date)
      .then((d) => live && (setDay(d), setError(null)))
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [sel, date, students]);

  const today = dateStr();
  const habit = habits.find((h) => h.id === sel) ?? null;

  const tap = async (studentId: string) => {
    if (!sel) return;
    try {
      const r = await boardApi.habitToggle(sel, studentId, date);
      setDay((d) =>
        d && {
          ...d,
          doneCount: r.doneCount,
          total: r.total,
          entries: d.entries.map((e) => (e.studentId === studentId ? { ...e, done: r.done } : e)),
        },
      );
      setVersion((v) => v + 1);
    } catch (e) {
      setError(errorText(e));
    }
  };

  if (!cls) return <p className="bd-note">학급이 없습니다.</p>;

  if (view === 'manage') {
    return (
      <HabitManager
        classId={cls.id}
        onClose={() => {
          setView('day');
          void loadHabits();
        }}
      />
    );
  }

  return (
    <div>
      {error && <p className="bd-error">{error}</p>}
      <div className="bd-bar">
        {habits.map((h) => (
          <button key={h.id} className={sel === h.id ? 'bd-opt on' : 'bd-opt'} aria-pressed={sel === h.id} onClick={() => setSel(h.id)}>
            {h.icon} {h.title}
          </button>
        ))}
        <span className="bd-grow" />
        <button className="bd-small" onClick={() => setView('manage')}>
          습관 관리
        </button>
      </div>

      {habits.length === 0 && <p className="bd-note">습관이 없습니다. 「습관 관리」에서 만들어 보세요 (예: 사물함 정리하기, 고운 말 쓰기).</p>}

      {habit && (
        <>
          <div className="bd-bar">
            <div className="bd-seg" role="group" aria-label="보기">
              <button className={view === 'day' ? 'bd-opt on' : 'bd-opt'} aria-pressed={view === 'day'} onClick={() => setView('day')}>
                오늘 체크
              </button>
              <button className={view === 'week' ? 'bd-opt on' : 'bd-opt'} aria-pressed={view === 'week'} onClick={() => setView('week')}>
                주간 보기
              </button>
            </div>
            {view === 'day' && (
              <div className="bd-date" role="group" aria-label="날짜">
                <button className="bd-step" disabled={date <= addDays(today, -MAX_BACK)} onClick={() => setDate(addDays(date, -1))} aria-label="전날">
                  ◀
                </button>
                <span className="bd-step-val">{date === today ? '오늘' : shortDate(date)}</span>
                <button className="bd-step" disabled={date >= today} onClick={() => setDate(addDays(date, 1))} aria-label="다음날">
                  ▶
                </button>
              </div>
            )}
            {view === 'day' && day && (
              <strong className="bd-count">
                {day.doneCount}/{day.total}명 했어요
              </strong>
            )}
          </div>

          {view === 'day' ? (
            day ? (
              <div className="bd-names bd-habit-grid">
                {day.entries.map((e) => (
                  <button key={e.studentId} className={e.done ? 'bd-name done' : 'bd-name'} aria-pressed={e.done} onClick={() => tap(e.studentId)}>
                    <span className="bd-light" aria-hidden="true">
                      {e.done ? '●' : '○'}
                    </span>
                    {e.name}
                    <small>{e.done ? '했어요' : '아직'}</small>
                  </button>
                ))}
              </div>
            ) : (
              <p className="bd-note">불러오는 중…</p>
            )
          ) : (
            <div className="bd-teacher">
              <HabitWeekTable habit={habit} version={version} />
            </div>
          )}
        </>
      )}
    </div>
  );
}

interface Draft {
  id?: string;
  title: string;
  icon: string;
  startsOn: string;
  endsOn: string;
  active: boolean;
}

function HabitManager({ classId, onClose }: { classId: string; onClose: () => void }) {
  const [list, setList] = useState<Habit[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    try {
      setList(await boardApi.habitList(classId, true));
    } catch (e) {
      setError(errorText(e));
    }
  }, [classId]);
  useEffect(() => {
    void reload();
  }, [reload]);

  const save = async () => {
    if (!draft) return;
    try {
      await boardApi.habitSave({
        id: draft.id,
        classId,
        title: draft.title,
        icon: draft.icon,
        startsOn: draft.startsOn || null,
        endsOn: draft.endsOn || null,
        active: draft.active,
      });
      setDraft(null);
      setError(null);
      await reload();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const remove = async (h: Habit) => {
    if (!window.confirm(`'${h.title}' 습관과 체크 기록을 모두 지울까요? 되돌릴 수 없습니다.`)) return;
    try {
      await boardApi.habitDelete(h.id);
      await reload();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setDraft((d) => (d ? { ...d, [k]: v } : d));

  return (
    <div className="bd-teacher">
      <div className="bd-bar">
        <h2 className="bd-h3 bd-grow">습관 관리 (교사용)</h2>
        <button className="bd-small" onClick={onClose}>
          칠판으로 돌아가기
        </button>
      </div>
      {error && <p className="bd-error">{error}</p>}
      {!draft ? (
        <>
          <button className="bd-small" onClick={() => setDraft({ title: '', icon: '', startsOn: '', endsOn: '', active: true })}>
            + 새 습관
          </button>
          <ul className="bd-t-list">
            {list.map((h) => (
              <li key={h.id}>
                <span className="bd-grow">
                  {h.icon} {h.title}
                  {!h.active && ' (쉬는 중)'}
                  {(h.startsOn || h.endsOn) && ` · ${h.startsOn ?? ''} ~ ${h.endsOn ?? ''}`}
                </span>
                <button
                  className="bd-small"
                  onClick={() => setDraft({ id: h.id, title: h.title, icon: h.icon, startsOn: h.startsOn ?? '', endsOn: h.endsOn ?? '', active: h.active })}
                >
                  수정
                </button>
                <button className="bd-small" onClick={() => remove(h)}>
                  삭제
                </button>
              </li>
            ))}
          </ul>
        </>
      ) : (
        <form
          className="bd-t-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <div className="bd-t-row">
            <label>
              이름
              <input value={draft.title} maxLength={30} onChange={(e) => set('title', e.target.value)} />
            </label>
            <label>
              이모지 (선택)
              <input value={draft.icon} maxLength={8} onChange={(e) => set('icon', e.target.value)} style={{ width: '6em' }} />
            </label>
          </div>
          <div className="bd-t-row">
            <label>
              시작일 (선택)
              <input type="date" value={draft.startsOn} onChange={(e) => set('startsOn', e.target.value)} />
            </label>
            <label>
              끝나는 날 (선택)
              <input type="date" value={draft.endsOn} onChange={(e) => set('endsOn', e.target.value)} />
            </label>
            <label className="bd-t-check">
              <input type="checkbox" checked={draft.active} onChange={(e) => set('active', e.target.checked)} />
              진행 중
            </label>
          </div>
          <div className="bd-t-row">
            <button className="bd-small" type="submit">
              저장
            </button>
            <button className="bd-small" type="button" onClick={() => setDraft(null)}>
              취소
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
