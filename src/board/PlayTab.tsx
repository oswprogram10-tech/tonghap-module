import { useEffect, useRef, useState } from 'react';
import { curriculum } from '../curriculum';
import { errorText, type ClassInfo } from '../records/api';
import { GROUP_LABEL, PLACE_LABEL, boardApi, type Play } from './api';
import PlayManager from './PlayManager';
import { cryptoRng, filterPlays, pickPlay, randInt } from './random';
import { beep } from './sound';

type Mode = 'roulette' | 'list' | 'card' | 'manage';
const SPIN_MS = 2500;
const rng = cryptoRng();

interface Props {
  plays: Play[];
  custom: Play[];
  topicId: string | null;
  cls: ClassInfo | null;
  onCustomChanged: () => Promise<void>;
  onTeams: () => void;
  onTimer: (minutes: number) => void;
}

export default function PlayTab({ plays, custom, topicId, cls, onCustomChanged, onTeams, onTimer }: Props) {
  const [mode, setMode] = useState<Mode>('roulette');
  const [place, setPlace] = useState('all');
  const [onlyTopic, setOnlyTopic] = useState(false);
  const [short, setShort] = useState(false);
  const [spinning, setSpinning] = useState(false);
  const [shown, setShown] = useState<Play | null>(null);
  const [landed, setLanded] = useState<Play | null>(null);
  const [card, setCard] = useState<Play | null>(null);
  const [error, setError] = useState<string | null>(null);
  const lastId = useRef<string | null>(null);
  const timerId = useRef<number | null>(null);

  const topicTitle = curriculum.topicBooks.find((t) => t.id === topicId)?.title ?? null;
  const filtered = filterPlays(plays, { place, topicBookId: topicId, onlyTopic, short });

  useEffect(
    () => () => {
      if (timerId.current) window.clearInterval(timerId.current);
    },
    [],
  );

  const spin = () => {
    if (spinning || filtered.length === 0) return;
    const result = pickPlay(filtered, lastId.current, rng);
    if (!result) return;
    setLanded(null);
    setSpinning(true);
    const start = Date.now();
    timerId.current = window.setInterval(() => {
      if (Date.now() - start >= SPIN_MS) {
        if (timerId.current) window.clearInterval(timerId.current);
        timerId.current = null;
        lastId.current = result.id;
        setShown(result);
        setLanded(result);
        setSpinning(false);
        beep(1, 1000);
      } else {
        setShown(filtered[randInt(rng, filtered.length)]);
      }
    }, 90);
  };

  const choose = (p: Play) => {
    lastId.current = p.id;
    setCard(p);
    setMode('card');
    if (cls) boardApi.playLog(cls.id, p.id, p.minutes).catch((e) => setError(errorText(e)));
  };

  if (mode === 'manage') {
    return <PlayManager custom={custom} onChanged={onCustomChanged} onClose={() => setMode('roulette')} />;
  }

  if (mode === 'card' && card) {
    return (
      <div className="bd-card">
        {error && <p className="bd-error">{error}</p>}
        <h2 className="bd-h2">{card.name}</h2>
        <p className="bd-chips">
          <span className="bd-chip">{PLACE_LABEL[card.place]}</span>
          <span className="bd-chip">{GROUP_LABEL[card.group]}</span>
          <span className="bd-chip">{card.minutes}분</span>
        </p>
        <ol className="bd-rules">
          {card.rules.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ol>
        <div className="bd-safety">
          <strong>안전 수칙</strong>
          <ul>
            {card.safety.map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ul>
        </div>
        {card.materials.length > 0 && <p className="bd-materials">준비물: {card.materials.join(', ')}</p>}
        <div className="bd-actions">
          <button className="bd-big" onClick={onTeams}>
            팀 나누기로
          </button>
          <button className="bd-big" onClick={() => onTimer(card.minutes)}>
            타이머로 ({card.minutes}분)
          </button>
          <button className="bd-big bd-ghost" onClick={() => setMode('roulette')}>
            다른 놀이
          </button>
        </div>
      </div>
    );
  }

  return (
    <div>
      <div className="bd-bar">
        <div className="bd-seg" role="group" aria-label="장소">
          {[
            ['all', '전체'],
            ['outdoor', '실외'],
            ['indoor', '실내'],
          ].map(([v, label]) => (
            <button key={v} className={place === v ? 'bd-opt on' : 'bd-opt'} aria-pressed={place === v} onClick={() => setPlace(v)}>
              {label}
            </button>
          ))}
        </div>
        {topicTitle && (
          <button className={onlyTopic ? 'bd-opt on' : 'bd-opt'} aria-pressed={onlyTopic} onClick={() => setOnlyTopic((v) => !v)}>
            이번 주제책({topicTitle})
          </button>
        )}
        <button className={short ? 'bd-opt on' : 'bd-opt'} aria-pressed={short} onClick={() => setShort((v) => !v)}>
          15분 이하
        </button>
        <span className="bd-grow" />
        <button className={mode === 'list' ? 'bd-opt on' : 'bd-opt'} onClick={() => setMode(mode === 'list' ? 'roulette' : 'list')}>
          놀이 목록
        </button>
        <button className="bd-small" onClick={() => setMode('manage')}>
          놀이 관리
        </button>
      </div>
      {error && <p className="bd-error">{error}</p>}
      <p className="bd-note">놀이 {filtered.length}개</p>

      {mode === 'list' ? (
        <div className="bd-grid">
          {filtered.map((p) => (
            <button key={p.id} className="bd-play-card" onClick={() => choose(p)}>
              <strong>{p.name}</strong>
              <span>
                {PLACE_LABEL[p.place]} · {p.minutes}분
              </span>
            </button>
          ))}
        </div>
      ) : (
        <div className="bd-roulette">
          <div className={spinning ? 'bd-window spin' : 'bd-window'} aria-live="polite">
            {shown ? shown.name : '돌려 볼까요?'}
          </div>
          {landed && !spinning && (
            <p className="bd-chips">
              <span className="bd-chip">{PLACE_LABEL[landed.place]}</span>
              <span className="bd-chip">{GROUP_LABEL[landed.group]}</span>
              <span className="bd-chip">{landed.minutes}분</span>
            </p>
          )}
          <div className="bd-actions">
            <button className="bd-big" disabled={spinning || filtered.length === 0} onClick={spin}>
              {landed ? '다시' : '돌리기'}
            </button>
            {landed && !spinning && (
              <button className="bd-big bd-primary" onClick={() => choose(landed)}>
                이 놀이로
              </button>
            )}
          </div>
          {filtered.length === 0 && <p className="bd-note">조건에 맞는 놀이가 없습니다. 걸러내기를 풀어 보세요.</p>}
        </div>
      )}
    </div>
  );
}
