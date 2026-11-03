import { useCallback, useEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { curriculum } from '../curriculum';
import { errorText, recordsApi, type ClassInfo, type Student } from '../records/api';
import { BOARD_CLASS_EVENT, BUILTIN_PLAYS, boardApi, dateStr, type Play } from './api';
import HabitTab from './HabitTab';
import PlayTab from './PlayTab';
import TeamTab from './TeamTab';
import TimerTab from './TimerTab';
import { formatClock } from './random';
import { setSoundOn, soundOn } from './sound';
import { useTimer } from './useTimer';
import './board.css';

type Tab = 'play' | 'team' | 'timer' | 'habit';
const TABS: { id: Tab; label: string }[] = [
  { id: 'play', label: '놀이' },
  { id: 'team', label: '팀 나누기' },
  { id: 'timer', label: '타이머' },
  { id: 'habit', label: '습관 신호등' },
];

interface Absent {
  date: string;
  studentIds: string[];
}

/** 전자칠판용 창. 메인 창과 같은 번들을 `#/board`로 연다. */
export default function BoardApp() {
  const [tab, setTab] = useState<Tab>('play');
  const [cls, setCls] = useState<ClassInfo | null>(null);
  const [students, setStudents] = useState<Student[]>([]);
  const [topicId, setTopicId] = useState<string | null>(null);
  const [custom, setCustom] = useState<Play[]>([]);
  const [absent, setAbsent] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [sound, setSound] = useState(soundOn());
  const [fullscreen, setFullscreen] = useState(false);
  const timer = useTimer(180);

  const load = useCallback(async () => {
    try {
      const [savedClass, savedTopic, savedAbsent, classes, customPlays] = await Promise.all([
        recordsApi.settingsGet<string>('currentClassId'),
        recordsApi.settingsGet<string>('currentTopicBookId'),
        recordsApi.settingsGet<Absent>('boardAbsent'),
        recordsApi.classList(),
        boardApi.customPlays(),
      ]);
      const c = classes.find((x) => x.id === savedClass) ?? classes[0] ?? null;
      const list = c ? await recordsApi.studentList(c.id, false) : [];
      setCls(c);
      setStudents(list);
      setTopicId(c && savedTopic && curriculum.topicBooks.some((t) => t.id === savedTopic && t.grade === c.grade) ? savedTopic : null);
      setCustom(customPlays);
      const today = dateStr();
      setAbsent(savedAbsent && savedAbsent.date === today ? savedAbsent.studentIds.filter((id) => list.some((s) => s.id === id)) : []);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void load();
    if (!isTauri()) return;
    const un = listen(BOARD_CLASS_EVENT, () => void load());
    return () => {
      void un.then((f) => f());
    };
  }, [load]);

  const changeAbsent = (ids: string[]) => {
    setAbsent(ids);
    recordsApi.settingsSet('boardAbsent', { date: dateStr(), studentIds: ids }).catch((e) => setError(errorText(e)));
  };

  const toggleFullscreen = async () => {
    try {
      const w = getCurrentWindow();
      const next = !(await w.isFullscreen());
      await w.setFullscreen(next);
      setFullscreen(next);
    } catch (e) {
      setError(errorText(e));
    }
  };
  const closeBoard = () => {
    getCurrentWindow()
      .close()
      .catch((e) => setError(errorText(e)));
  };

  const plays = [...BUILTIN_PLAYS, ...custom];
  const goTimer = (minutes: number) => {
    timer.setTotal(minutes * 60);
    setTab('timer');
  };
  const timerShown = timer.running || timer.remaining < timer.total;

  // 타이머 끝남 알림이 떠 있을 때 화면 깜빡임은 어느 탭에서든 보인다.
  const flashRef = useRef<HTMLDivElement>(null);

  return (
    <div className={timer.finished ? 'bd bd-flash' : 'bd'} ref={flashRef}>
      <header className="bd-top">
        <h1 className="bd-title">바슬즐 칠판</h1>
        <nav className="bd-tabs" aria-label="칠판 메뉴">
          {TABS.map((t) => (
            <button key={t.id} className={tab === t.id ? 'bd-tab on' : 'bd-tab'} aria-current={tab === t.id} onClick={() => setTab(t.id)}>
              {t.label}
            </button>
          ))}
        </nav>
        {timerShown && tab !== 'timer' && (
          <button className={timer.remaining <= 10 ? 'bd-mini bd-mini-hot' : 'bd-mini'} onClick={() => setTab('timer')} aria-label="남은 시간">
            ⏱ {formatClock(timer.remaining)}
            {!timer.running && ' (멈춤)'}
          </button>
        )}
        <div className="bd-top-end">
          <button
            className="bd-small"
            aria-pressed={sound}
            onClick={() => {
              setSoundOn(!sound);
              setSound(!sound);
            }}
          >
            소리 {sound ? '켜짐' : '꺼짐'}
          </button>
          <button className="bd-small" onClick={toggleFullscreen}>
            {fullscreen ? '창으로' : '전체 화면'}
          </button>
          <button className="bd-small" onClick={closeBoard}>
            닫기
          </button>
        </div>
      </header>

      {error && <p className="bd-error">{error}</p>}
      {!loaded ? (
        <p className="bd-note">불러오는 중…</p>
      ) : !cls ? (
        <p className="bd-note">메인 창에서 학급을 먼저 만들어 주세요. 놀이와 타이머는 학급 없이도 쓸 수 있습니다.</p>
      ) : null}

      {loaded && (
        <main className="bd-main">
          <section hidden={tab !== 'play'}>
            <PlayTab plays={plays} custom={custom} topicId={topicId} cls={cls} onCustomChanged={load} onTeams={() => setTab('team')} onTimer={goTimer} />
          </section>
          <section hidden={tab !== 'team'}>
            <TeamTab students={students} absent={absent} onAbsent={changeAbsent} cls={cls} />
          </section>
          <section hidden={tab !== 'timer'}>
            <TimerTab timer={timer} />
          </section>
          <section hidden={tab !== 'habit'}>
            <HabitTab cls={cls} students={students} />
          </section>
        </main>
      )}

      {timer.finished && (
        <div className="bd-finish" role="alert">
          <p>시간이 다 됐어요!</p>
          <button className="bd-big" onClick={timer.dismiss}>
            확인
          </button>
        </div>
      )}
    </div>
  );
}
