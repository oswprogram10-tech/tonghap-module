import { useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { saveFile } from '../lib/dialogs';
import { formatBytes } from '../diag/estimate';
import { errorText } from '../records/api';
import { defaultFileName, supportApi, type SupportItem } from './api';
import { SUPPORT_EMAIL } from '../about/content';

type Phase =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'preview'; items: SupportItem[] }
  | { kind: 'creating'; items: SupportItem[] }
  | { kind: 'done'; path: string };

/** 「문의용 진단 파일 만들기」: 들어가는 파일 목록을 보여 주고, 선생님이 「만들기」를 누르면 zip을 만든다. */
export default function SupportExport() {
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' });
  const [error, setError] = useState<string | null>(null);

  if (!isTauri()) {
    return <p className="diag-note">진단 파일은 설치된 앱에서만 만들 수 있습니다.</p>;
  }

  const start = async () => {
    setError(null);
    setPhase({ kind: 'loading' });
    try {
      setPhase({ kind: 'preview', items: await supportApi.preview() });
    } catch (e) {
      setError(errorText(e));
      setPhase({ kind: 'idle' });
    }
  };

  const create = async (items: SupportItem[]) => {
    setError(null);
    try {
      const path = await saveFile({ defaultPath: defaultFileName(), filters: [{ name: '진단 파일', extensions: ['zip'] }] });
      if (!path) return;
      setPhase({ kind: 'creating', items });
      const r = await supportApi.create(path);
      setPhase({ kind: 'done', path: r.path });
    } catch (e) {
      setError(errorText(e));
      setPhase({ kind: 'preview', items });
    }
  };

  return (
    <div className="support-export">
      {phase.kind === 'idle' || phase.kind === 'loading' ? (
        <button className="diag-primary" onClick={start} disabled={phase.kind === 'loading'}>
          {phase.kind === 'loading' ? '확인하는 중…' : '문의용 진단 파일 만들기'}
        </button>
      ) : phase.kind === 'done' ? (
        <>
          <p className="diag-note" role="status">
            진단 파일을 만들었습니다. 이 파일을 문의처({SUPPORT_EMAIL})로 보내 주세요. 앱이 자동으로 보내지는 않습니다.
          </p>
          <p className="diag-hint">
            <code>{phase.path}</code>
          </p>
          <button
            className="diag-secondary"
            onClick={() => revealItemInDir(phase.path).catch((e) => setError(`폴더를 열 수 없습니다: ${errorText(e)}`))}
          >
            폴더 열기
          </button>{' '}
          <button className="diag-secondary" onClick={() => setPhase({ kind: 'idle' })}>
            닫기
          </button>
        </>
      ) : (
        <>
          <p className="diag-note">아래 파일만 진단 파일에 들어갑니다.</p>
          <ul className="support-list">
            {phase.items.map((it) => (
              <li key={it.name}>
                <code>{it.name}</code> · {it.label} · {formatBytes(it.bytes)}
              </li>
            ))}
          </ul>
          <p className="diag-hint">들어가지 않는 것: 기록 데이터(DB), 사진·음성, 백업 파일, 평어·리포트, 학생 명단. 학생 이름은 오류 기록에서도 가려집니다.</p>
          <button className="diag-primary" disabled={phase.kind === 'creating'} onClick={() => create(phase.items)}>
            {phase.kind === 'creating' ? '만드는 중…' : '만들기'}
          </button>{' '}
          <button className="diag-secondary" disabled={phase.kind === 'creating'} onClick={() => setPhase({ kind: 'idle' })}>
            취소
          </button>
        </>
      )}
      {error && <p className="diag-error">진단 파일을 만들 수 없습니다: {error}</p>}
    </div>
  );
}
