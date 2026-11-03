import { useEffect, useState } from 'react';
import { recordsApi } from '../records/api';
import { BACKUP_REMINDER_KEY, REMINDER_DAYS, backupApi, todayKey, type BackupStatus } from './api';

/** 마지막 백업이 없거나 7일이 지났고 그 뒤 새 기록이 있으면 관찰 기록 화면 위에 보여 준다. */
export default function BackupReminder({ onGoBackup }: { onGoBackup: () => void }) {
  const [status, setStatus] = useState<BackupStatus | null>(null);
  const [hiddenToday, setHiddenToday] = useState(true);

  useEffect(() => {
    let live = true;
    Promise.all([backupApi.status(), recordsApi.settingsGet<string>(BACKUP_REMINDER_KEY)])
      .then(([s, dismissedOn]) => {
        if (!live) return;
        setStatus(s);
        setHiddenToday(dismissedOn === todayKey());
      })
      .catch((e) => console.error('백업 상태 확인 실패', e));
    return () => {
      live = false;
    };
  }, []);

  if (!status || hiddenToday || status.observationsSince <= 0) return null;
  if (status.lastBackup) {
    const age = Date.now() - new Date(status.lastBackup.at).getTime();
    if (Number.isFinite(age) && age < REMINDER_DAYS * 24 * 3600 * 1000) return null;
  }

  const dismiss = () => {
    setHiddenToday(true);
    recordsApi.settingsSet(BACKUP_REMINDER_KEY, todayKey()).catch((e) => console.error('설정 저장 실패', e));
  };

  return (
    <section className="backup-reminder" role="status" aria-label="백업 알림">
      <p>
        {status.lastBackup ? `마지막 백업 후 기록 ${status.observationsSince}개가 늘었습니다.` : `아직 백업한 적이 없습니다. 기록 ${status.observationsSince}개가 이 PC에만 있습니다.`}
      </p>
      <div className="backup-reminder-actions">
        <button className="diag-primary" onClick={onGoBackup}>
          지금 백업
        </button>
        <button className="diag-secondary" onClick={dismiss}>
          오늘은 닫기
        </button>
      </div>
    </section>
  );
}
