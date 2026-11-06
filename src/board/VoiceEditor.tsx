import { useCallback, useEffect, useRef, useState } from 'react';
import { toWav16k } from '../stt/convert';
import { trimWav, type CueId } from './cues';
import { playVoice } from './sound';
import { CUES, clearVoice, saveVoice, teacherUrl } from './voice';

const MAX_SECONDS = 10;

/** 문구마다 듣기 / 녹음 / 기본으로. 녹음은 최대 10초, 저장은 앱 안(기록과 함께 백업된다). */
export default function VoiceEditor() {
  const [mine, setMine] = useState<Partial<Record<CueId, boolean>>>({});
  const [recId, setRecId] = useState<CueId | null>(null);
  const [seconds, setSeconds] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const recorder = useRef<MediaRecorder | null>(null);
  const stream = useRef<MediaStream | null>(null);
  const tick = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    const next: Partial<Record<CueId, boolean>> = {};
    for (const c of CUES) next[c.id] = (await teacherUrl(c.id)) !== null;
    setMine(next);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const release = () => {
    if (tick.current !== null) window.clearInterval(tick.current);
    tick.current = null;
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

  const stop = () => {
    if (recorder.current && recorder.current.state !== 'inactive') recorder.current.stop();
  };

  const record = async (id: CueId) => {
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
    const rec = new MediaRecorder(s);
    const chunks: Blob[] = [];
    const startedAt = Date.now();
    rec.ondataavailable = (ev) => {
      if (ev.data.size > 0) chunks.push(ev.data);
    };
    rec.onstop = () => {
      const blob = new Blob(chunks, { type: rec.mimeType || 'audio/webm' });
      release();
      setRecId(null);
      if (blob.size === 0) {
        setMessage('녹음된 내용이 없습니다.');
        return;
      }
      void (async () => {
        try {
          const wav = trimWav(await toWav16k(await blob.arrayBuffer()), MAX_SECONDS);
          await saveVoice(id, wav);
          await refresh();
          setMessage('녹음을 저장했습니다.');
        } catch (e) {
          setMessage(e instanceof Error ? e.message : typeof e === 'string' ? e : '녹음을 저장하지 못했습니다.');
        }
      })();
    };
    recorder.current = rec;
    rec.start();
    setSeconds(0);
    setRecId(id);
    tick.current = window.setInterval(() => {
      const sec = (Date.now() - startedAt) / 1000;
      setSeconds(Math.floor(sec));
      if (sec >= MAX_SECONDS) stop();
    }, 200);
  };

  const reset = async (id: CueId) => {
    setMessage(null);
    try {
      await clearVoice(id);
      await refresh();
      setMessage('기본 음성으로 되돌렸습니다.');
    } catch (e) {
      setMessage(typeof e === 'string' ? e : '되돌리지 못했습니다.');
    }
  };

  const listen = async (id: CueId) => {
    setMessage(null);
    if (!(await playVoice(id))) setMessage('소리를 재생하지 못했습니다.');
  };

  return (
    <div className="bd-voice" aria-label="안내 음성 바꾸기">
      <p className="bd-note">문구마다 선생님 목소리로 녹음해 바꿀 수 있습니다(최대 {MAX_SECONDS}초). 녹음은 이 컴퓨터의 앱 안에만 저장됩니다.</p>
      <ul className="bd-voice-list">
        {CUES.map((c) => (
          <li key={c.id} className="bd-voice-row">
            <div className="bd-voice-text">
              <strong>{c.text}</strong>
              <span className="bd-voice-tag">{mine[c.id] ? '내 목소리' : '기본 음성'}</span>
            </div>
            <div className="bd-voice-btns">
              <button className="bd-opt" onClick={() => void listen(c.id)} disabled={recId !== null}>
                듣기
              </button>
              {recId === c.id ? (
                <button className="bd-opt on" onClick={stop}>
                  ■ 정지 ({seconds}초)
                </button>
              ) : (
                <button className="bd-opt" onClick={() => void record(c.id)} disabled={recId !== null}>
                  ● 녹음
                </button>
              )}
              <button className="bd-opt" onClick={() => void reset(c.id)} disabled={!mine[c.id] || recId !== null}>
                기본으로
              </button>
            </div>
          </li>
        ))}
      </ul>
      {message && (
        <p className="bd-note" role="status">
          {message}
        </p>
      )}
    </div>
  );
}
