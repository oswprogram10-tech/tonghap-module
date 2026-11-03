import { useEffect, useRef, useState } from 'react';

export interface Recording {
  id: string;
  blob: Blob;
  mime: string;
  seconds: number;
  url: string;
}

/** MediaRecorder 녹음기. 정지하면 녹음 하나를 onDone으로 넘기고 마이크를 바로 놓는다. */
export default function AudioRecorder({ onDone }: { onDone: (r: Recording) => void }) {
  const [recording, setRecording] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const recorder = useRef<MediaRecorder | null>(null);
  const stream = useRef<MediaStream | null>(null);
  const timer = useRef<number | null>(null);

  const release = () => {
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    stream.current?.getTracks().forEach((t) => t.stop());
    stream.current = null;
  };

  useEffect(
    () => () => {
      if (recorder.current && recorder.current.state !== 'inactive') {
        recorder.current.onstop = null;
        recorder.current.stop();
      }
      release();
    },
    [],
  );

  const start = async () => {
    setMessage(null);
    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
      setMessage('이 환경에서는 녹음을 쓸 수 없습니다.');
      return;
    }
    let s: MediaStream;
    try {
      s = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (e) {
      const denied = e instanceof DOMException && (e.name === 'NotAllowedError' || e.name === 'SecurityError');
      setMessage(
        denied
          ? '마이크 사용이 허용되지 않았습니다. 윈도우 설정의 개인 정보 > 마이크에서 앱의 마이크 사용을 켜 주세요.'
          : '마이크를 찾을 수 없습니다. 마이크가 연결되어 있는지 확인해 주세요.',
      );
      return;
    }
    stream.current = s;
    const preferred = 'audio/webm;codecs=opus';
    const rec = MediaRecorder.isTypeSupported(preferred) ? new MediaRecorder(s, { mimeType: preferred }) : new MediaRecorder(s);
    const chunks: Blob[] = [];
    const startedAt = Date.now();
    rec.ondataavailable = (ev) => {
      if (ev.data.size > 0) chunks.push(ev.data);
    };
    rec.onstop = () => {
      const mime = rec.mimeType || 'audio/webm';
      const blob = new Blob(chunks, { type: mime });
      release();
      setRecording(false);
      if (blob.size === 0) {
        setMessage('녹음된 내용이 없습니다.');
        return;
      }
      onDone({
        id: `${startedAt}`,
        blob,
        mime,
        seconds: Math.max(1, Math.round((Date.now() - startedAt) / 1000)),
        url: URL.createObjectURL(blob),
      });
    };
    recorder.current = rec;
    rec.start();
    setSeconds(0);
    setRecording(true);
    timer.current = window.setInterval(() => setSeconds(Math.round((Date.now() - startedAt) / 1000)), 500);
  };

  const stop = () => {
    if (recorder.current && recorder.current.state !== 'inactive') recorder.current.stop();
  };

  return (
    <div className="rec-recorder">
      {recording ? (
        <button className="diag-primary rec-recording" onClick={stop}>
          ■ 정지 ({seconds}초)
        </button>
      ) : (
        <button className="diag-secondary" onClick={start}>
          ● 음성 메모
        </button>
      )}
      {message && <p className="diag-error">{message}</p>}
    </div>
  );
}
