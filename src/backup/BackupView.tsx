import { useCallback, useEffect, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { pickFile, saveFile } from '../lib/dialogs';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { formatBytes } from '../diag/estimate';
import { errorText } from '../records/api';
import { backupApi, defaultBackupName, type BackupProgress, type BackupResult, type BackupStatus, type BackupSummary } from './api';

const STAGE_LABEL: Record<BackupProgress['stage'], string> = {
  snapshot: '데이터 복사하는 중',
  hashing: '파일 확인하는 중',
  writing: '백업 파일 쓰는 중',
  reading: '백업 파일 읽는 중',
  verifying: '내용 검증하는 중',
  encrypting: '사진·음성 보호하는 중',
  swapping: '데이터 바꾸는 중',
};

const FILTERS = [{ name: '바슬즐 백업', extensions: ['tonghap'] }];

function ProgressBar({ p }: { p: BackupProgress }) {
  const pct = p.total > 0 ? Math.min(100, Math.round((p.done / p.total) * 100)) : null;
  return (
    <div className="backup-progress" role="status">
      <span>
        {STAGE_LABEL[p.stage]}
        {pct !== null && ` · ${pct}%`}
      </span>
      {pct !== null ? <progress max={100} value={pct} /> : <progress />}
    </div>
  );
}

function when(iso: string) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('ko-KR', { dateStyle: 'long', timeStyle: 'short' });
}

/**
 * `restoreOnly`: 기록을 열 수 없을 때의 안내 화면에서 쓴다. 백업 만들기는 숨기고 복원만 보여 준다.
 */
export default function BackupView({ onRestored, restoreOnly = false }: { onRestored: () => void; restoreOnly?: boolean }) {
  if (!isTauri()) {
    return (
      <main className="diag">
        <p className="diag-note">백업은 설치된 앱에서만 동작합니다. 브라우저 미리보기에서는 사용할 수 없습니다.</p>
      </main>
    );
  }
  return <Backup onRestored={onRestored} restoreOnly={restoreOnly} />;
}

function Backup({ onRestored, restoreOnly }: { onRestored: () => void; restoreOnly: boolean }) {
  const [status, setStatus] = useState<BackupStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [progress, setProgress] = useState<BackupProgress | null>(null);
  const [result, setResult] = useState<BackupResult | null>(null);
  const [createError, setCreateError] = useState<string | null>(null);

  const [file, setFile] = useState<string | null>(null);
  const [rpw, setRpw] = useState('');
  const [summary, setSummary] = useState<BackupSummary | null>(null);
  const [rProgress, setRProgress] = useState<BackupProgress | null>(null);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const [restored, setRestored] = useState(false);

  const loadStatus = useCallback(() => {
    backupApi.status().then(setStatus).catch((e) => setCreateError(errorText(e)));
  }, []);
  useEffect(() => {
    if (!restoreOnly) loadStatus();
  }, [loadStatus, restoreOnly]);

  const pwError = pw.length > 0 && (pw.trim().length === 0 || [...pw].length < 8) ? '암호는 8자 이상이어야 합니다.' : pw2.length > 0 && pw !== pw2 ? '암호 확인이 일치하지 않습니다.' : null;
  const canCreate = !busy && [...pw].length >= 8 && pw.trim().length > 0 && pw === pw2;

  const create = async () => {
    setCreateError(null);
    setResult(null);
    const path = await saveFile({ title: '백업 파일 저장', defaultPath: defaultBackupName(), filters: FILTERS });
    if (!path) return;
    setBusy(true);
    setProgress({ stage: 'snapshot', done: 0, total: 0 });
    try {
      setResult(await backupApi.create(path, pw, setProgress));
      setPw('');
      setPw2('');
      loadStatus();
    } catch (e) {
      setCreateError(errorText(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const pick = async () => {
    const picked = await pickFile({ title: '백업 파일 선택', filters: FILTERS });
    if (!picked) return;
    setFile(picked);
    setSummary(null);
    setRestoreError(null);
    setRestored(false);
  };

  const inspect = async () => {
    if (!file) return;
    setBusy(true);
    setRestoreError(null);
    setSummary(null);
    try {
      setSummary(await backupApi.inspect(file, rpw));
    } catch (e) {
      setRestoreError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const restore = async () => {
    if (!file || !summary) return;
    const message = restoreOnly
      ? '열 수 없는 기록 파일은 지우지 않고 보관 폴더(unreadable-날짜)에 그대로 둡니다. 백업 내용으로 새로 시작합니다.\n계속할까요?'
      : '현재 데이터는 보관 폴더로 옮겨 두고 백업 내용으로 바꿉니다.\n계속할까요?';
    if (!window.confirm(message)) return;
    setBusy(true);
    setRestoreError(null);
    setRProgress({ stage: 'reading', done: 0, total: 0 });
    try {
      await backupApi.restore(file, rpw, setRProgress);
      setRestored(true);
      setSummary(null);
      setRpw('');
      if (!restoreOnly) loadStatus();
      onRestored();
    } catch (e) {
      setRestoreError(errorText(e));
    } finally {
      setBusy(false);
      setRProgress(null);
    }
  };

  return (
    <main className="diag backup">
      {!restoreOnly && (
      <section className="diag-section">
        <h2>백업 만들기</h2>
        <p className="diag-hint">
          {status?.lastBackup
            ? `마지막 백업: ${when(status.lastBackup.at)} · 그 뒤 새 기록 ${status.observationsSince}개`
            : `아직 백업한 적이 없습니다. 기록 ${status?.observationsSince ?? 0}개가 이 PC에만 있습니다.`}
        </p>
        <div className="backup-form">
          <label className="rec-field">
            암호 (8자 이상)
            <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="new-password" disabled={busy} />
          </label>
          <label className="rec-field">
            암호 확인
            <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} autoComplete="new-password" disabled={busy} />
          </label>
        </div>
        {pwError && <p className="diag-error">{pwError}</p>}
        <p className="backup-warn">암호를 잊으면 누구도 복원할 수 없습니다. USB나 개인 저장 공간 등 이 PC가 아닌 곳에 보관하세요.</p>
        <button className="diag-primary" onClick={create} disabled={!canCreate}>
          백업 파일 저장
        </button>
        {progress && <ProgressBar p={progress} />}
        {createError && <p className="diag-error">{createError}</p>}
        {result && (
          <div className="backup-done" role="status">
            <p>
              백업을 마쳤습니다. 학급 {result.counts.classes} · 학생 {result.counts.students} · 기록 {result.counts.observations} · 첨부{' '}
              {result.counts.attachments} · {formatBytes(result.bytes)}
            </p>
            <p className="diag-hint">{result.path}</p>
            {result.missing.length > 0 && <p className="diag-error">파일을 찾지 못해 넣지 못한 첨부가 {result.missing.length}개 있습니다.</p>}
            <button className="diag-secondary" onClick={() => revealItemInDir(result.path)}>
              폴더 열기
            </button>
          </div>
        )}
      </section>
      )}

      <section className="diag-section">
        <h2>복원하기</h2>
        <p className="diag-hint">
          {restoreOnly
            ? '백업 파일의 내용으로 기록을 새로 시작합니다. 열 수 없는 기록 파일은 보관 폴더에 그대로 남습니다.'
            : '백업 파일의 내용으로 현재 데이터를 바꿉니다. 현재 데이터는 보관 폴더(최근 3개)에 남습니다.'}
        </p>
        <button className="diag-secondary" onClick={pick} disabled={busy}>
          백업 파일 선택
        </button>
        {file && <p className="diag-hint">{file}</p>}
        {file && (
          <>
            <div className="backup-form">
              <label className="rec-field">
                백업 암호
                <input type="password" value={rpw} onChange={(e) => { setRpw(e.target.value); setSummary(null); }} autoComplete="off" disabled={busy} />
              </label>
            </div>
            <button className="diag-primary" onClick={inspect} disabled={busy || rpw.length === 0}>
              내용 확인
            </button>
          </>
        )}
        {summary && (
          <div className="backup-summary">
            <table className="sys-table">
              <tbody>
                <tr><th>만든 날짜</th><td>{when(summary.createdAt)}</td></tr>
                <tr><th>앱 버전</th><td>{summary.appVersion}</td></tr>
                <tr><th>학급 · 학생</th><td>{summary.counts.classes}개 · {summary.counts.students}명</td></tr>
                <tr><th>기록 · 첨부</th><td>{summary.counts.observations}개 · {summary.counts.attachments}개</td></tr>
                <tr><th>총 크기</th><td>{formatBytes(summary.totalBytes)}</td></tr>
              </tbody>
            </table>
            <button className="diag-primary" onClick={restore} disabled={busy}>
              이 백업으로 복원
            </button>
          </div>
        )}
        {rProgress && <ProgressBar p={rProgress} />}
        {restoreError && <p className="diag-error">{restoreError}</p>}
        {restored && (
          <p className="backup-done" role="status">
            복원을 마쳤습니다. 학급과 기록을 다시 불러왔습니다.
          </p>
        )}
      </section>
    </main>
  );
}
