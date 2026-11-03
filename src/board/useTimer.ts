import { useCallback, useEffect, useRef, useState } from 'react';
import { beep } from './sound';

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

  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => {
      const left = (endAt.current - Date.now()) / 1000;
      if (left <= 0) {
        window.clearInterval(id);
        setRemaining(0);
        setRunning(false);
        setFinished(true);
        beep(3);
      } else {
        setRemaining(left);
      }
    }, 200);
    return () => window.clearInterval(id);
  }, [running]);

  const setTotal = useCallback((seconds: number) => {
    const s = Math.max(1, Math.min(seconds, 99 * 60 + 59));
    setTotalState(s);
    setRemaining(s);
    setRunning(false);
    setFinished(false);
  }, []);

  const start = useCallback(() => {
    if (remaining <= 0) return;
    endAt.current = Date.now() + remaining * 1000;
    setFinished(false);
    setRunning(true);
  }, [remaining]);

  const pause = useCallback(() => {
    setRemaining(Math.max(0, (endAt.current - Date.now()) / 1000));
    setRunning(false);
  }, []);

  const reset = useCallback(() => {
    setRunning(false);
    setFinished(false);
    setRemaining(total);
  }, [total]);

  const dismiss = useCallback(() => {
    setFinished(false);
    setRemaining(total);
  }, [total]);

  return { total, remaining, running, finished, setTotal, start, pause, reset, dismiss };
}
