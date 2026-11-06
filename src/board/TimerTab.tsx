import { useState } from 'react';
import { SOUND_MODES, type SoundMode } from './cues';
import { formatClock } from './random';
import VoiceEditor from './VoiceEditor';
import type { TimerState } from './useTimer';

const PRESETS = [1, 3, 5, 10, 15];
const R = 140;
const C = 2 * Math.PI * R;

export default function TimerTab({ timer, sound, onSound }: { timer: TimerState; sound: SoundMode; onSound: (m: SoundMode) => void }) {
  const [editing, setEditing] = useState(false);
  const { total, remaining, running, setTotal } = timer;
  const idle = !running && remaining === total;
  const hot = remaining <= 10 && (running || remaining < total);
  const frac = total > 0 ? Math.max(0, Math.min(1, remaining / total)) : 0;
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  const adjust = (dm: number, ds: number) => setTotal(Math.max(1, minutes * 60 + seconds + dm * 60 + ds));

  return (
    <div className="bd-timer">
      <div className="bd-bar" role="group" aria-label="시간 고르기">
        {PRESETS.map((m) => (
          <button key={m} className={total === m * 60 ? 'bd-opt on' : 'bd-opt'} aria-pressed={total === m * 60} onClick={() => setTotal(m * 60)}>
            {m}분
          </button>
        ))}
      </div>

      <div className={hot ? 'bd-dial hot' : 'bd-dial'}>
        <svg viewBox="0 0 320 320" aria-hidden="true">
          <circle cx="160" cy="160" r={R} className="bd-ring-bg" />
          <circle cx="160" cy="160" r={R} className="bd-ring" strokeDasharray={C} strokeDashoffset={C * (1 - frac)} transform="rotate(-90 160 160)" />
        </svg>
        <div className="bd-time" role="timer" aria-live="off">
          {formatClock(remaining)}
        </div>
      </div>

      <div className="bd-actions">
        {running ? (
          <button className="bd-big" onClick={timer.pause}>
            일시정지
          </button>
        ) : (
          <button className="bd-big bd-primary" onClick={timer.start} disabled={remaining <= 0}>
            {idle ? '시작' : '계속'}
          </button>
        )}
        <button className="bd-big bd-ghost" onClick={timer.reset} disabled={idle}>
          재설정
        </button>
      </div>

      <div className="bd-sound" role="group" aria-label="소리 방식">
        <span className="bd-sound-label">소리</span>
        {SOUND_MODES.map((m) => (
          <button key={m.id} className={sound === m.id ? 'bd-opt on' : 'bd-opt'} aria-pressed={sound === m.id} onClick={() => onSound(m.id)}>
            {m.label}
          </button>
        ))}
        <button className="bd-opt" aria-expanded={editing} onClick={() => setEditing(!editing)}>
          안내 음성 바꾸기
        </button>
      </div>
      {editing && <VoiceEditor />}

      {idle && (
        <div className="bd-custom" aria-label="직접 시간 정하기">
          <div>
            <button className="bd-step" onClick={() => adjust(-1, 0)} aria-label="분 줄이기">
              −
            </button>
            <span className="bd-step-val">{minutes}분</span>
            <button className="bd-step" onClick={() => adjust(1, 0)} aria-label="분 늘리기">
              +
            </button>
          </div>
          <div>
            <button className="bd-step" onClick={() => adjust(0, -10)} aria-label="초 줄이기">
              −
            </button>
            <span className="bd-step-val">{seconds}초</span>
            <button className="bd-step" onClick={() => adjust(0, 10)} aria-label="초 늘리기">
              +
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
