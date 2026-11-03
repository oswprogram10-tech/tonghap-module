import { convertFileSrc } from '@tauri-apps/api/core';
import Transcript from '../stt/Transcript';
import type { Attachment } from './api';

/** 저장된 첨부: 사진 썸네일과 음성 재생. 파일은 asset 프로토콜(미디어 폴더 한정)로 연다. */
export default function AttachmentView({
  attachments,
  onRemove,
}: {
  attachments: Attachment[];
  onRemove?: (a: Attachment) => void;
}) {
  if (attachments.length === 0) return null;
  return (
    <div className="rec-attachments">
      {attachments.map((a) => (
        <div key={a.id} className={a.kind === 'photo' ? 'rec-attach rec-attach-photo' : 'rec-attach rec-attach-audio'}>
          {a.kind === 'photo' ? (
            a.mime === 'image/heic' ? (
              <span className="rec-chip-static">사진(HEIC) {a.relPath.split('/').pop()}</span>
            ) : (
              <img src={convertFileSrc(a.absPath)} alt="관찰 사진" loading="lazy" />
            )
          ) : (
            <audio controls preload="none" src={convertFileSrc(a.absPath)} />
          )}
          {a.kind === 'audio' && <Transcript attachment={a} />}
          {onRemove && (
            <button className="diag-secondary" onClick={() => onRemove(a)}>
              {a.kind === 'photo' ? '사진 빼기' : '녹음 빼기'}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
