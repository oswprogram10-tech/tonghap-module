import { isTauri } from '@tauri-apps/api/core';
import SupportExport from '../support/SupportExport';
import BackupView from './BackupView';

/**
 * 암호화된 기록을 이 윈도우 계정에서 열 수 없을 때 앱 전체를 대신하는 안내 화면.
 * 다른 탭은 보이지 않고, 백업 파일로 복원하거나 문의용 진단 파일을 만들 수만 있다. 새 기록을 만들지 않는다.
 */
export default function LockedView({ reason, onRestored }: { reason: string | null; onRestored: () => void }) {
  // .app이 3행 격자라서 한 덩어리로 감싼다.
  return (
    <div className="locked-wrap">
      <main className="diag locked" aria-label="기록을 열 수 없음">
        <section className="diag-section">
          <h2>이 윈도우 계정에서 기록을 열 수 없습니다</h2>
          <p className="diag-error" role="alert">
            이 윈도우 계정에서 기록을 열 수 없습니다. 다른 윈도우 계정으로 로그인했거나, 관리자가 윈도우 암호를 재설정한 경우일 수 있습니다. 원래 계정으로 로그인해 보거나, 백업 파일로
            복원하세요.
          </p>
          <p className="diag-hint">기존 기록 파일은 지우거나 바꾸지 않고 그대로 두었습니다.</p>
          {reason && <p className="diag-hint">자세한 이유: {reason}</p>}
        </section>
      </main>
      {isTauri() && <BackupView restoreOnly onRestored={onRestored} />}
      <main className="diag locked-support" aria-label="문의하기">
        <section className="diag-section">
          <h2>문의하기</h2>
          <SupportExport />
        </section>
      </main>
    </div>
  );
}
