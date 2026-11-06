import { useCallback, useEffect, useRef, useState } from 'react';
import { dueCue, markPassed, type CueId } from './cues';
import { playCue } from './sound';

export interface TimerState {
  /** 설정한 전체 시간(초). */
  total: number;
  /** 남은 시간(초, 소수 가능). */
  remaining: number;
  running: boolean;
  /** 끝나서 알림을 보여 주는 중. */
  finished: boolean;
  setTotal: (seconds: number) => void;
  start: () => void;
  pause: () => void;
  reset: () => void;
  dismiss: () => void;
}

/** 끝나는 시각을 기준으로 계산해서, 탭을 옮기거나 화면이 느려도 시간이 틀어지지 않는다. */
export function useTimer(initialSeconds = 180): TimerState {
  const [total, setTotalState] = useState(initialSeconds);
  const [remaining, setRemaining] = useState(initialSeconds);
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState(false);
  const endAt = useRef(0);
  /** 이번 회차에서 이미 낸 안내(일시정지·다시 시작해도 같은 안내가 두 번 나오지 않게). */
  const played = useRef<Set<CueId>>(new Set());

  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => {
      const left = (endAt.current - Date.now()) / 1000;
      if (left <= 0) {
        window.clearInterval(id);
        setRemaining(0);
        setRunning(false);
        setFinished(true);
        played.current = new Set();
        void playCue('timer-end');
      } else {
        setRemaining(left);
        const due = dueCue(left, total, played.current);
        played.current = due.played;
        if (due.cue) void playCue(due.cue);
      }
    }, 200);
    return () => window.clearInterval(id);
  }, [running, total]);

  const setTotal = useCallback((seconds: number) => {
    const s = Math.max(1, Math.min(seconds, 99 * 60 + 59));
    setTotalState(s);
    setRemaining(s);
    setRunning(false);
    setFinished(false);
    played.current = new Set();
  }, []);

  const start = useCallback(() => {
    if (remaining <= 0) return;
    endAt.current = Date.now() + remaining * 1000;
    played.current = markPassed(remaining, total, played.current);
    setFinished(false);
    setRunning(true);
  }, [remaining, total]);

  const pause = useCallback(() => {
    setRemaining(Math.max(0, (endAt.current - Date.now()) / 1000));
    setRunning(false);
  }, []);

  const reset = useCallback(() => {
    setRunning(false);
    setFinished(false);
    setRemaining(total);
    played.current = new Set();
  }, [total]);

  const dismiss = useCallback(() => {
    setFinished(false);
    setRemaining(total);
    played.current = new Set();
  }, [total]);

  return { total, remaining, running, finished, setTotal, start, pause, reset, dismiss };
}
