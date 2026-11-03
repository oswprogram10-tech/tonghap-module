import { useEffect, useRef, useState } from 'react';
import type { Attachment } from '../records/api';
import { sttApi, requestTranscript } from './api';

const errorText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e));

/**
 * 음성 메모의 자막 영역(기록 수정 창·포트폴리오 공통).
 * 상태는 Rust 대기열이 가지고 있고, `stt://updated` 이벤트가 오면 이 첨부만 다시 불러온다.
 */
export default function Transcript({ attachment }: { attachment: Attachment }) {
  const [att, setAtt] = useState(attachment);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const idRef = useRef(attachment.id);
  idRef.current = attachment.id;

  useEffect(() => setAtt(attachment), [attachment]);

  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | undefined;
    const refetch = () =>
      sttApi
        .attachmentGet(idRef.current)
        .then((a) => live && setAtt(a))
        .catch((e) => console.warn('자막 상태 새로 고침 실패', e));
    sttApi
      .onUpdated((u) => {
        if (u.attachmentId === idRef.current) void refetch();
      })
      .then((fn) => {
        if (!live) return fn();
        unlisten = fn;
        // 구독 전에 끝난 작업을 놓치지 않도록 한 번 맞춰 본다.
        if (att.transcriptStatus === 'queued' || att.transcriptStatus === 'running') void refetch();
      })
      .catch((e) => console.warn('자막 이벤트 구독 실패', e));
    return () => {
      live = false;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const make = async () => {
    setError(null);
    setBusy(true);
    try {
      await requestTranscript(att);
      setAtt(await sttApi.attachmentGet(att.id));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const remake = () => {
    const msg = att.transcriptEdited
      ? '고친 자막이 새 자막으로 바뀝니다. 다시 만들까요?'
      : '지금 자막이 새 자막으로 바뀝니다. 다시 만들까요?';
    if (window.confirm(msg)) void make();
  };

  const save = async () => {
    setError(null);
    setBusy(true);
    try {
      setAtt(await sttApi.setTranscript(att.id, draft));
      setEditing(false);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const status = att.transcriptStatus;
  return (
    <div className="stt" data-status={status}>
      {status === 'none' && !editing && (
        <button className="diag-secondary" onClick={make} disabled={busy}>
          자막 만들기
        </button>
      )}
      {status === 'queued' && <p className="stt-wait">자막 대기 중</p>}
      {status === 'running' && (
        <p className="stt-wait">
          자막 만드는 중<span className="stt-dots" aria-hidden="true" />
        </p>
      )}
      {status === 'failed' && (
        <>
          <ErrorText text={att.transcriptError ?? '자막을 만들지 못했습니다'} />
          <button className="diag-secondary" onClick={make} disabled={busy}>
            다시 시도
          </button>
        </>
      )}
      {status === 'done' && !editing && (
        <>
          <p className="rec-transcript stt-text">
            {att.transcript}
            {att.transcriptEdited && <span className="stt-badge">고침</span>}
          </p>
          <div className="stt-actions">
            <button
              className="diag-secondary"
              onClick={() => {
                setDraft(att.transcript ?? '');
                setEditing(true);
              }}
              disabled={busy}
            >
              고치기
            </button>
            <button className="diag-secondary" onClick={remake} disabled={busy}>
              다시 만들기
            </button>
          </div>
        </>
      )}
      {editing && (
        <>
          <textarea
            className="stt-edit"
            rows={4}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            aria-label="자막 고치기"
          />
          <div className="stt-actions">
            <button className="diag-primary" onClick={save} disabled={busy}>
              저장
            </button>
            <button className="diag-secondary" onClick={() => setEditing(false)} disabled={busy}>
              취소
            </button>
          </div>
        </>
      )}
      {error && <p className="diag-error">{error}</p>}
    </div>
  );
}

/** 첫 줄(한국어 안내)만 보여 주고, 엔진 원문은 「자세히」에 접어 둔다. */
function ErrorText({ text }: { text: string }) {
  const [head, ...rest] = text.split('\n');
  const detail = rest.join('\n').trim();
  return (
    <div className="diag-error">
      <p>{head}</p>
      {detail && (
        <details className="stt-error-detail">
          <summary>자세히</summary>
          <pre>{detail}</pre>
        </details>
      )}
    </div>
  );
}
