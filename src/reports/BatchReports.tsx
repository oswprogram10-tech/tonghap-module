import { useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { errorText, recordsApi, type ClassInfo } from '../records/api';
import { topicTitle, topicsOfGrade } from '../records/util';
import { reportApi, type ReportDraft } from './api';
import ReportSheet, { sheetData } from './ReportSheet';
import { joinPath, nextPaint, renderPng, reportFileName } from './renderPng';

interface Failure {
  label: string;
  reason: string;
}

/** 학급 전원의 리포트를 한 번에 만들어 고른 폴더에 저장한다. 하나가 실패해도 나머지를 계속한다. */
export default function BatchReports({ cls, topicId, onClose }: { cls: ClassInfo; topicId: string | null; onClose: () => void }) {
  const topics = useMemo(() => topicsOfGrade(cls.grade), [cls.grade]);
  const [topic, setTopic] = useState<string | null>(topicId);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number; current: string } | null>(null);
  const [failures, setFailures] = useState<Failure[]>([]);
  const [okCount, setOkCount] = useState<number | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [current, setCurrent] = useState<ReportDraft | null>(null);
  const sheetRef = useRef<HTMLDivElement>(null);

  const run = async () => {
    setError(null);
    setFailures([]);
    setOkCount(null);
    try {
      const dir = await open({ directory: true, multiple: false, title: '리포트를 저장할 폴더' });
      if (typeof dir !== 'string') return;
      setFolder(dir);
      setRunning(true);
      const students = await recordsApi.studentList(cls.id, false);
      const period = await reportApi.defaultPeriod(cls.id, topic);
      const fails: Failure[] = [];
      let ok = 0;
      for (const [i, s] of students.entries()) {
        const label = `${s.number}번 ${s.name}`;
        setProgress({ done: i, total: students.length, current: label });
        try {
          // 저장된 리포트가 있으면 그 선택을, 없으면 기본 선택을 쓴다.
          const d = await reportApi.draft(s.id, topic, period.from, period.to);
          setCurrent(d);
          await nextPaint();
          if (!sheetRef.current) throw new Error('리포트를 그리지 못했습니다');
          const bytes = await renderPng(sheetRef.current);
          await reportApi.writeFile(joinPath(dir, reportFileName(s.number, s.name, topicTitle(topic))), bytes);
          if (d.id) await reportApi.markExported(d.id);
          ok += 1;
        } catch (e) {
          fails.push({ label, reason: errorText(e) });
          setFailures([...fails]);
        }
      }
      setProgress({ done: students.length, total: students.length, current: '' });
      setOkCount(ok);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRunning(false);
      setCurrent(null);
    }
  };

  const sel = current && { observationIds: current.observationIds, attachmentIds: current.attachmentIds, quoteAttachmentIds: current.quoteAttachmentIds, message: current.message };
  const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;

  return (
    <div className="rec-modal" role="dialog" aria-modal="true" aria-label="학급 리포트 일괄 만들기">
      <div className="rec-modal-body">
        <div className="rec-row rec-wrap">
          <h2 className="rec-grow">학급 리포트 일괄 만들기</h2>
          <button className="diag-secondary" onClick={onClose} disabled={running}>
            닫기
          </button>
        </div>
        <p className="diag-hint">
          학급 학생 전원의 리포트를 폴더 하나에 저장합니다. 이미 편집해 저장한 리포트는 그 선택을 쓰고, 없으면 기본 선택(단독 기록 사진만)으로 만듭니다.
          이미지는 고른 폴더에만 저장되며 어디로도 보내지 않습니다.
        </p>
        {error && <p className="diag-error">{error}</p>}
        <label className="rec-field">
          주제책
          <select value={topic ?? ''} disabled={running} onChange={(e) => setTopic(e.target.value || null)}>
            <option value="">선택 안 함</option>
            {topics.map((t) => (
              <option key={t.id} value={t.id}>
                {t.semester}학기 · {t.title}
              </option>
            ))}
          </select>
        </label>
        <div className="rec-row rec-wrap rp-actions">
          <button className="diag-primary" disabled={running} onClick={run}>
            {running ? '만드는 중…' : '폴더를 골라 만들기'}
          </button>
        </div>

        {progress && (
          <div className="rp-progress" aria-live="polite">
            <div className="rp-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
              <div className="rp-bar-fill" style={{ width: `${pct}%` }} />
            </div>
            <p>
              {progress.done}/{progress.total}명 {running && progress.current ? `· ${progress.current} 만드는 중…` : ''}
            </p>
          </div>
        )}

        {okCount !== null && !running && (
          <p className="diag-note" role="status">
            {okCount}명의 리포트를 저장했습니다{folder ? `: ${folder}` : ''}
          </p>
        )}
        {failures.length > 0 && (
          <div className="rp-failures">
            <h3>저장하지 못한 리포트 ({failures.length})</h3>
            <ul>
              {failures.map((f) => (
                <li key={f.label}>
                  <strong>{f.label}</strong> — {f.reason}
                </li>
              ))}
            </ul>
          </div>
        )}

        <div className="rp-offscreen" aria-hidden="true">
          {current && sel && <ReportSheet ref={sheetRef} data={sheetData(current, sel)} />}
        </div>
      </div>
    </div>
  );
}
