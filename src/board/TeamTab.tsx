import { useEffect, useRef, useState } from 'react';
import type { ClassInfo, Student } from '../records/api';
import { cryptoRng, pickWeighted, present, randInt, splitTeams } from './random';
import { beep } from './sound';

const rng = cryptoRng();
const TEAMS = [
  { name: '토끼', icon: '🐰', color: '#fde2e2', edge: '#c0392b' },
  { name: '곰', icon: '🐻', color: '#fdebd0', edge: '#b9770e' },
  { name: '호랑이', icon: '🐯', color: '#fcf3cf', edge: '#9a7d0a' },
  { name: '펭귄', icon: '🐧', color: '#d6eaf8', edge: '#2471a3' },
  { name: '여우', icon: '🦊', color: '#e8daef', edge: '#7d3c98' },
  { name: '고래', icon: '🐳', color: '#d1f2eb', edge: '#148f77' },
];
const SPIN_MS = 2000;

interface Props {
  students: Student[];
  absent: string[];
  onAbsent: (ids: string[]) => void;
  cls: ClassInfo | null;
}

export default function TeamTab({ students, absent, onAbsent, cls }: Props) {
  const [teamCount, setTeamCount] = useState(2);
  const [teams, setTeams] = useState<Student[][] | null>(null);
  const [tagCount, setTagCount] = useState(1);
  const [spinName, setSpinName] = useState<string | null>(null);
  const [tagged, setTagged] = useState<Student[] | null>(null);
  const [spinning, setSpinning] = useState(false);
  const history = useRef<string[][]>([]);
  const timerId = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timerId.current) window.clearInterval(timerId.current);
    },
    [],
  );
  // 학급이 바뀌면 이전 결과를 지운다.
  useEffect(() => {
    setTeams(null);
    setTagged(null);
    history.current = [];
  }, [cls?.id]);

  const here = present(students, absent);
  const toggle = (id: string) => onAbsent(absent.includes(id) ? absent.filter((x) => x !== id) : [...absent, id]);

  const divide = () => {
    // 팀 이름·색은 나눌 때마다 같은 순서(1팀부터)로 붙인다.
    setTeams(splitTeams(here, teamCount, rng));
    beep(1, 700);
  };

  const pick = () => {
    if (spinning || here.length === 0) return;
    const ids = pickWeighted(
      here.map((s) => s.id),
      tagCount,
      history.current,
      rng,
    );
    const chosen = ids.map((id) => here.find((s) => s.id === id)!);
    setTagged(null);
    setSpinning(true);
    const start = Date.now();
    timerId.current = window.setInterval(() => {
      if (Date.now() - start >= SPIN_MS) {
        if (timerId.current) window.clearInterval(timerId.current);
        timerId.current = null;
        history.current = [...history.current, ids].slice(-10);
        setTagged(chosen);
        setSpinName(null);
        setSpinning(false);
        beep(1, 1000);
      } else {
        setSpinName(here[randInt(rng, here.length)].name);
      }
    }, 80);
  };

  if (students.length === 0) {
    return <p className="bd-note">이 학급에 학생이 없습니다. 메인 창의 포트폴리오에서 학급 관리로 명단을 넣어 주세요.</p>;
  }

  return (
    <div>
      <h2 className="bd-h3">
        오늘 참여: {here.length}명 <span className="bd-hint">(빠진 친구 이름을 누르세요)</span>
      </h2>
      <div className="bd-names">
        {students.map((s) => {
          const out = absent.includes(s.id);
          return (
            <button key={s.id} className={out ? 'bd-name out' : 'bd-name'} aria-pressed={out} onClick={() => toggle(s.id)}>
              {s.name}
              {out && <small>빠짐</small>}
            </button>
          );
        })}
      </div>

      <section className="bd-block">
        <h2 className="bd-h3">팀 나누기</h2>
        <div className="bd-bar">
          {[2, 3, 4, 5, 6].map((n) => (
            <button key={n} className={teamCount === n ? 'bd-opt on' : 'bd-opt'} aria-pressed={teamCount === n} onClick={() => setTeamCount(n)}>
              {n}팀
            </button>
          ))}
          <button className="bd-big bd-primary" disabled={here.length === 0} onClick={divide}>
            {teams ? '다시 나누기' : '나누기'}
          </button>
        </div>
        {teams && (
          <div className="bd-teams">
            {teams.map((t, i) => {
              const st = TEAMS[i % TEAMS.length];
              return (
                <div key={i} className="bd-team" style={{ background: st.color, borderColor: st.edge }}>
                  <h3>
                    {i + 1}팀 {st.icon} {st.name} <small>({t.length}명)</small>
                  </h3>
                  <p>{t.map((s) => s.name).join(', ')}</p>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="bd-block">
        <h2 className="bd-h3">술래 뽑기</h2>
        <div className="bd-bar">
          {[1, 2, 3, 4, 5].map((n) => (
            <button key={n} className={tagCount === n ? 'bd-opt on' : 'bd-opt'} aria-pressed={tagCount === n} onClick={() => setTagCount(n)}>
              {n}명
            </button>
          ))}
          <button className="bd-big bd-primary" disabled={spinning || here.length === 0} onClick={pick}>
            뽑기
          </button>
        </div>
        <div className={spinning ? 'bd-window spin' : 'bd-window'} aria-live="polite">
          {spinning ? spinName : tagged ? tagged.map((s) => s.name).join(', ') : '누가 될까요?'}
        </div>
      </section>
    </div>
  );
}
