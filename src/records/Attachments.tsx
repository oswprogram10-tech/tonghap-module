import { mediaUrl } from '../lib/media';
import Transcript from '../stt/Transcript';
import type { Attachment } from './api';

/** 저장된 첨부: 사진 썸네일과 음성 재생. 파일은 첨부 id로 tgmedia 프로토콜에 요청한다(디스크의 파일은 암호문). */
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
              <img src={mediaUrl(a.id)} alt="관찰 사진" loading="lazy" />
            )
          ) : (
            <audio controls preload="none" src={mediaUrl(a.id)} />
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
