import { useEffect, useState } from 'react';
import { recordsApi } from '../records/api';
import { MODEL_MISSING, sttApi } from './api';

const KEY = 'sttBannerDismissed';

/** 기록 화면 위쪽: 자동 자막이 켜져 있는데 모델이 없을 때 한 번 안내한다. */
export default function ModelBanner() {
  const [show, setShow] = useState(false);

  useEffect(() => {
    let live = true;
    Promise.all([sttApi.status(), recordsApi.settingsGet<boolean>(KEY)])
      .then(([st, dismissed]) => live && setShow(st.auto && !st.modelReady && dismissed !== true))
      .catch((e) => console.warn('자막 모델 상태 확인 실패', e));
    return () => {
      live = false;
    };
  }, []);

  if (!show) return null;
  return (
    <div className="stt-banner" role="status">
      <span>{MODEL_MISSING}</span>
      <button
        className="diag-secondary"
        onClick={() => {
          setShow(false);
          recordsApi.settingsSet(KEY, true).catch((e) => console.warn('설정 저장 실패', e));
        }}
      >
        다시 보지 않기
      </button>
    </div>
  );
}
