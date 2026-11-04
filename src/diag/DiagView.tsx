import { useCallback, useEffect, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { STT_MODELS, sttApi, type SttStatus } from '../stt/api';
import {
  EVAL_MODEL_IDS,
  diagApi,
  type DownloadProgress,
  type EngineInfo,
  type LlmBench,
  type ModelStatus,
  type SttBench,
  type SystemReport,
} from './api';
import {
  CLASS_SIZE,
  CLIP_SECONDS,
  GRADE_LABEL,
  bestLlm,
  estimateStt,
  formatBytes,
  formatSeconds,
  type LlmEstimate,
  type SttEstimate,
} from './estimate';

type StepStatus = 'pending' | 'running' | 'ok' | 'error' | 'skipped';
interface Step {
  key: string;
  label: string;
  status: StepStatus;
  detail?: string;
}

const STEP_ICON: Record<StepStatus, string> = {
  pending: '○',
  running: '…',
  ok: '✓',
  error: '✕',
  skipped: '–',
};

const STEP_TEXT: Record<StepStatus, string> = {
  pending: '대기',
  running: '진행 중',
  ok: '완료',
  error: '실패',
  skipped: '건너뜀',
};

const SUBTITLE_IDS: string[] = STT_MODELS.map((m) => m.id);

const errorText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e));

export default function DiagView() {
  if (!isTauri()) {
    return (
      <main className="diag">
        <p className="diag-note">기기 점검은 설치된 앱에서만 동작합니다. 브라우저 미리보기에서는 사용할 수 없습니다.</p>
      </main>
    );
  }
  return <DiagPanel />;
}

function DiagPanel() {
  const [sys, setSys] = useState<SystemReport | null>(null);
  const [sysError, setSysError] = useState<string | null>(null);
  const [engines, setEngines] = useState<EngineInfo | null>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress | undefined>>({});
  const [modelErrors, setModelErrors] = useState<Record<string, string | undefined>>({});
  const [steps, setSteps] = useState<Step[]>([]);
  const [running, setRunning] = useState(false);
  const [llmResults, setLlmResults] = useState<LlmBench[]>([]);
  const [stt, setStt] = useState<SttBench | null>(null);
  const [memo, setMemo] = useState('');
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [sttStatus, setSttStatus] = useState<SttStatus | null>(null);
  const [sttError, setSttError] = useState<string | null>(null);

  const refreshModels = useCallback(() => {
    diagApi.models().then(setModels).catch((e) => setSysError(errorText(e)));
  }, []);

  useEffect(() => {
    diagApi.system().then(setSys).catch((e) => setSysError(errorText(e)));
    diagApi.engines().then(setEngines).catch((e) => setSysError(errorText(e)));
    refreshModels();
    sttApi.status().then(setSttStatus).catch((e) => setSttError(errorText(e)));
  }, [refreshModels]);

  const busyModel = Object.values(progress).some(Boolean);

  const download = async (id: string) => {
    setModelErrors((m) => ({ ...m, [id]: undefined }));
    setProgress((p) => ({ ...p, [id]: { stage: 'downloading', downloaded: 0, total: 0, bytesPerSec: 0 } }));
    try {
      await diagApi.download(id, (pr) => setProgress((p) => ({ ...p, [id]: pr })));
    } catch (e) {
      setModelErrors((m) => ({ ...m, [id]: errorText(e) }));
    } finally {
      setProgress((p) => ({ ...p, [id]: undefined }));
      refreshModels();
    }
  };

  const importFile = async (m: ModelStatus) => {
    const picked = await open({ multiple: false, directory: false, title: `${m.fileName} 파일 선택` });
    if (typeof picked !== 'string') return;
    setModelErrors((e) => ({ ...e, [m.id]: undefined }));
    setProgress((p) => ({ ...p, [m.id]: { stage: 'verifying' } }));
    try {
      await diagApi.importModel(m.id, picked);
    } catch (e) {
      setModelErrors((er) => ({ ...er, [m.id]: errorText(e) }));
    } finally {
      setProgress((p) => ({ ...p, [m.id]: undefined }));
      refreshModels();
    }
  };

  const refreshStt = () => sttApi.status().then(setSttStatus).catch((e) => setSttError(errorText(e)));
  const chooseSttModel = (id: string) => {
    setSttError(null);
    sttApi.setModel(id).then(refreshStt).catch((e) => setSttError(errorText(e)));
  };
  const toggleSttAuto = () => {
    setSttError(null);
    sttApi.setAuto(!(sttStatus?.auto ?? true)).then(refreshStt).catch((e) => setSttError(errorText(e)));
  };

  const ready = (id: string) => models.find((m) => m.id === id)?.ready ?? false;
  const llmModel = ['qwen2.5-1.5b-q4km', 'qwen2.5-0.5b-q4km'].find(ready) ?? null;
  const sttModel = ready('whisper-base') ? 'whisper-base' : null;

  const runBench = async () => {
    setRunning(true);
    setSavedPath(null);
    setLlmResults([]);
    setStt(null);
    const plan: Step[] = [
      { key: 'gpu', label: '언어 모델 · GPU(Vulkan)', status: 'pending' },
      { key: 'cpu', label: '언어 모델 · CPU', status: 'pending' },
      { key: 'stt', label: '음성 인식 · CPU', status: 'pending' },
    ];
    setSteps(plan);
    const update = (key: string, patch: Partial<Step>) =>
      setSteps((s) => s.map((st) => (st.key === key ? { ...st, ...patch } : st)));

    const llm: LlmBench[] = [];
    for (const mode of ['gpu', 'cpu'] as const) {
      if (!llmModel) {
        update(mode, { status: 'skipped', detail: '언어 모델 파일이 없습니다' });
        continue;
      }
      if (mode === 'gpu' && !sys?.vulkan.available) {
        update(mode, { status: 'skipped', detail: 'Vulkan GPU가 없어 건너뜀' });
        continue;
      }
      update(mode, { status: 'running' });
      try {
        const r = await diagApi.benchLlm(llmModel, mode);
        llm.push(r);
        setLlmResults([...llm]);
        update(mode, {
          status: 'ok',
          detail: `입력 ${r.promptTokensPerSec.toFixed(1)} 토큰/초 · 생성 ${r.genTokensPerSec.toFixed(1)} 토큰/초 (${r.backends || '백엔드 정보 없음'})`,
        });
      } catch (e) {
        update(mode, { status: 'error', detail: errorText(e) });
      }
    }

    if (!sttModel) {
      update('stt', { status: 'skipped', detail: '음성 인식 모델 파일이 없습니다' });
    } else {
      update('stt', { status: 'running' });
      try {
        const r = await diagApi.benchWhisper(sttModel);
        setStt(r);
        update('stt', {
          status: 'ok',
          detail: `${r.audioSeconds}초 음성을 ${(r.elapsedMs / 1000).toFixed(1)}초에 변환 (스레드 ${r.threads})`,
        });
      } catch (e) {
        update('stt', { status: 'error', detail: errorText(e) });
      }
    }
    setRunning(false);
  };

  const llmEstimate = bestLlm(llmResults);
  const sttEstimate = stt ? estimateStt(stt) : null;

  const save = async () => {
    setSaveError(null);
    const report = {
      generatedAt: new Date().toISOString(),
      memo,
      system: sys,
      engines,
      models: models.map(({ id, ready: r }) => ({ id, ready: r })),
      steps,
      llm: llmResults,
      stt,
      estimates: {
        llm: llmEstimate && { mode: llmEstimate.source.mode, perStudentSec: llmEstimate.perStudentSec, grade: llmEstimate.grade },
        stt: sttEstimate && { perClipSec: sttEstimate.perClipSec, grade: sttEstimate.grade },
      },
    };
    try {
      setSavedPath(await diagApi.saveReport(report, buildSummary(memo, sys, steps, llmEstimate, sttEstimate)));
    } catch (e) {
      setSaveError(errorText(e));
    }
  };

  return (
    <main className="diag">
      <p className="diag-note">
        이 PC에서 로컬 AI가 얼마나 빠르게 동작하는지 측정합니다. 측정 결과는 이 PC 안에만 저장됩니다.
      </p>
      {sysError && <p className="diag-error">{sysError}</p>}

      <section className="diag-section">
        <h2>1. 기기 정보</h2>
        {sys ? <SystemTable sys={sys} engines={engines} /> : <p>확인하는 중…</p>}
      </section>

      <section className="diag-section">
        <h2>2. 측정용 모델 준비</h2>
        <p className="diag-hint">
          언어 모델은 둘 중 하나만 있으면 됩니다(정밀 점검이 있으면 그쪽을 씁니다). 인터넷이 막힌 PC에서는 다른 PC에서
          받은 파일을 「파일 선택」으로 가져오세요.
        </p>
        <ul className="model-list">
          {models.filter((m) => !SUBTITLE_IDS.includes(m.id) && !EVAL_MODEL_IDS.includes(m.id)).map((m) => (
            <ModelRow
              key={m.id}
              model={m}
              progress={progress[m.id]}
              error={modelErrors[m.id]}
              disabled={busyModel || running}
              onDownload={() => download(m.id)}
              onImport={() => importFile(m)}
            />
          ))}
        </ul>
      </section>

      <section className="diag-section" aria-label="평어 초안 모델">
        <h2>평어 초안 모델</h2>
        <p className="diag-hint">
          「평어」 탭에서 언어 모델로 초안을 만들 때 씁니다(Qwen3 4B, 약 2.5GB). 이 PC 안에서만 실행되고 아이 기록은 밖으로 나가지 않습니다.
          학생 1명 초안에 30초~1분쯤 걸립니다. 모델이 없어도 템플릿 초안은 쓸 수 있어요.
        </p>
        <ul className="model-list">
          {models.filter((m) => EVAL_MODEL_IDS.includes(m.id)).map((m) => (
            <ModelRow
              key={m.id}
              model={m}
              progress={progress[m.id]}
              error={modelErrors[m.id]}
              disabled={busyModel || running}
              onDownload={() => download(m.id)}
              onImport={() => importFile(m)}
            />
          ))}
        </ul>
        {engines && !engines.llamaServer && <p className="diag-error">언어 모델 서버(llama-server)가 없습니다.</p>}
      </section>

      <section className="diag-section" aria-label="음성 자막">
        <h2>음성 자막</h2>
        <p className="diag-hint">
          관찰 기록의 음성 메모를 이 PC 안에서 글로 받아씁니다. 아이 목소리는 틀리게 받아쓸 수 있으니 기록 화면에서 고칠 수 있습니다.
        </p>
        <div className="stt-pick" role="group" aria-label="사용할 모델">
          {STT_MODELS.map((o) => (
            <button
              key={o.id}
              className="diag-secondary"
              aria-pressed={sttStatus?.modelId === o.id}
              onClick={() => chooseSttModel(o.id)}
            >
              {o.label} · {o.note}
            </button>
          ))}
        </div>
        <ul className="model-list">
          {models.filter((m) => SUBTITLE_IDS.includes(m.id)).map((m) => (
            <ModelRow
              key={m.id}
              model={m}
              progress={progress[m.id]}
              error={modelErrors[m.id]}
              disabled={busyModel || running}
              onDownload={() => download(m.id).then(refreshStt)}
              onImport={() => importFile(m).then(refreshStt)}
            />
          ))}
        </ul>
        <button className="diag-secondary" aria-pressed={sttStatus?.auto ?? true} onClick={toggleSttAuto}>
          자동 받아쓰기: {(sttStatus?.auto ?? true) ? '켜짐' : '꺼짐'}
        </button>
        {sttStatus && !sttStatus.engineReady && <p className="diag-error">자막 엔진(whisper-cli)이 없습니다.</p>}
        {sttError && <p className="diag-error">{sttError}</p>}
      </section>

      <section className="diag-section">
        <h2>3. 속도 측정</h2>
        <p className="diag-hint">모델 크기와 PC 성능에 따라 몇 분에서 15분 정도 걸립니다. 측정 중에는 다른 작업을 하지 마세요.</p>
        <button className="diag-primary" onClick={runBench} disabled={running || busyModel || (!llmModel && !sttModel)}>
          {running ? '측정 중…' : '측정 시작'}
        </button>
        {steps.length > 0 && (
          <ul className="step-list">
            {steps.map((s) => (
              <li key={s.key} className={`step step-${s.status}`}>
                <span className="step-icon">{STEP_ICON[s.status]}</span>
                <span className="step-label">{s.label}</span>
                {s.detail && <span className="step-detail">{s.detail}</span>}
              </li>
            ))}
          </ul>
        )}
      </section>

      {(llmEstimate || sttEstimate) && (
        <section className="diag-section">
          <h2>4. 판정 (추정)</h2>
          <ul className="verdicts">
            {llmEstimate && (
              <li className={`verdict verdict-${llmEstimate.grade}`}>
                <strong>생기부 평어 초안 (3B급 모델 기준)</strong>
                <span>
                  학생 1명 약 {formatSeconds(llmEstimate.perStudentSec)} · {CLASS_SIZE}명 약{' '}
                  {formatSeconds(llmEstimate.classMinutes * 60)} ({llmEstimate.source.mode === 'gpu' ? 'GPU' : 'CPU'} 기준)
                </span>
                <em>{GRADE_LABEL[llmEstimate.grade]}</em>
              </li>
            )}
            {sttEstimate && (
              <li className={`verdict verdict-${sttEstimate.grade}`}>
                <strong>음성 자막 (Whisper base)</strong>
                <span>
                  {CLIP_SECONDS}초 녹음 1개 약 {formatSeconds(sttEstimate.perClipSec)}
                </span>
                <em>{GRADE_LABEL[sttEstimate.grade]}</em>
              </li>
            )}
          </ul>
          <p className="diag-hint">
            측정한 작은 모델의 속도를 모델 크기에 비례해 환산한 값입니다. 실제 시간은 달라질 수 있습니다.
          </p>
        </section>
      )}

      <section className="diag-section">
        <h2>5. 결과 저장</h2>
        <label className="memo">
          <span>이 PC 구분 메모 (예: 본관 3층 1-2반 교사 PC)</span>
          <input value={memo} onChange={(e) => setMemo(e.target.value)} maxLength={100} />
        </label>
        <div className="save-row">
          <button className="diag-primary" onClick={save} disabled={running || !sys}>
            결과 저장
          </button>
          {savedPath && (
            <button className="diag-secondary" onClick={() => revealItemInDir(savedPath)}>
              저장 폴더 열기
            </button>
          )}
        </div>
        {savedPath && <p className="diag-hint">저장됨: {savedPath}</p>}
        {saveError && <p className="diag-error">{saveError}</p>}
      </section>
    </main>
  );
}

function SystemTable({ sys, engines }: { sys: SystemReport; engines: EngineInfo | null }) {
  const rows: [string, string, boolean?][] = [
    ['운영체제', `${sys.os} (${sys.arch})`],
    ['CPU', `${sys.cpuBrand || '알 수 없음'} · 물리 ${sys.physicalCores ?? '?'}코어 / 논리 ${sys.logicalCores}스레드`],
    ['메모리', `전체 ${formatBytes(sys.totalMemoryBytes)} · 사용 가능 ${formatBytes(sys.availableMemoryBytes)}`, sys.totalMemoryBytes < 8e9],
    ['디스크 여유', formatBytes(sys.diskFreeBytes), (sys.diskFreeBytes ?? Infinity) < 5e9],
    ['모델 폴더 쓰기', sys.modelsDirWritable ? '가능' : '불가', !sys.modelsDirWritable],
    ['WebView2', sys.webviewVersion ?? '알 수 없음'],
    [
      'GPU (Vulkan)',
      sys.vulkan.available
        ? sys.vulkan.devices.map((d) => `${d.name} (${d.deviceType}, Vulkan ${d.apiVersion}, 전용 메모리 ${formatBytes(d.deviceLocalBytes)})`).join(' / ')
        : `사용 불가: ${sys.vulkan.error ?? ''}`,
      !sys.vulkan.available,
    ],
    ['인터넷 (Hugging Face)', `${sys.network.ok ? '연결됨' : '연결 안 됨'} · ${sys.network.detail}`, !sys.network.ok],
    [
      '측정 엔진',
      engines
        ? `llama-bench ${engines.llamaBench ? '있음' : '없음'} · llama-server ${engines.llamaServer ? '있음' : '없음'} · whisper-cli ${engines.whisperCli ? '있음' : '없음'}${
            engines.versions ? ` (${Object.values(engines.versions).map((v) => v.tag).join(', ')})` : ''
          }`
        : '확인하는 중',
      engines ? !engines.llamaBench || !engines.llamaServer || !engines.whisperCli : false,
    ],
  ];
  return (
    <table className="sys-table">
      <tbody>
        {rows.map(([k, v, warn]) => (
          <tr key={k} className={warn ? 'warn' : undefined}>
            <th>{k}</th>
            <td>{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ModelRow(props: {
  model: ModelStatus;
  progress?: DownloadProgress;
  error?: string;
  disabled: boolean;
  onDownload: () => void;
  onImport: () => void;
}) {
  const { model: m, progress: p, error } = props;
  let status: string;
  if (p?.stage === 'downloading') {
    const pct = p.total ? (p.downloaded / p.total) * 100 : 0;
    status = `내려받는 중 ${pct.toFixed(0)}% · ${formatBytes(p.bytesPerSec)}/초`;
  } else if (p?.stage === 'verifying') {
    status = '파일 확인 중…';
  } else if (m.ready) {
    status = '준비됨';
  } else if (m.partialBytes > 0) {
    status = `일부 받음 (${formatBytes(m.partialBytes)}) · 이어받기 가능`;
  } else {
    status = '없음';
  }
  const pct = p?.stage === 'downloading' && p.total ? (p.downloaded / p.total) * 100 : null;
  return (
    <li className={m.ready ? 'model-row ready' : 'model-row'}>
      <div className="model-info">
        <strong>{m.label}</strong>
        <span>
          {m.fileName} · {formatBytes(m.size)} · {m.license}
        </span>
        <span className="model-status">{status}</span>
        {pct !== null && <progress max={100} value={pct} />}
        {error && <span className="diag-error">{error}</span>}
      </div>
      {!m.ready && (
        <div className="model-actions">
          <button className="diag-secondary" onClick={props.onDownload} disabled={props.disabled}>
            {m.partialBytes > 0 ? '이어받기' : '내려받기'}
          </button>
          <button className="diag-secondary" onClick={props.onImport} disabled={props.disabled}>
            파일 선택
          </button>
        </div>
      )}
    </li>
  );
}

function buildSummary(
  memo: string,
  sys: SystemReport | null,
  steps: Step[],
  llm: LlmEstimate | null,
  stt: SttEstimate | null,
): string {
  const lines = [
    '바슬즐 탐험대 기기 점검 결과',
    `측정 시각: ${new Date().toLocaleString('ko-KR')}`,
    `메모: ${memo || '(없음)'}`,
    '',
  ];
  if (sys) {
    lines.push(
      `운영체제: ${sys.os} (${sys.arch})`,
      `CPU: ${sys.cpuBrand} · 물리 ${sys.physicalCores ?? '?'}코어 / 논리 ${sys.logicalCores}스레드`,
      `메모리: 전체 ${formatBytes(sys.totalMemoryBytes)} · 사용 가능 ${formatBytes(sys.availableMemoryBytes)}`,
      `디스크 여유: ${formatBytes(sys.diskFreeBytes)}`,
      `GPU(Vulkan): ${
        sys.vulkan.available ? sys.vulkan.devices.map((d) => `${d.name} (${d.deviceType})`).join(', ') : `사용 불가 - ${sys.vulkan.error}`
      }`,
      `인터넷(Hugging Face): ${sys.network.ok ? '연결됨' : '연결 안 됨'} - ${sys.network.detail}`,
      `WebView2: ${sys.webviewVersion ?? '알 수 없음'}`,
      '',
    );
  }
  lines.push('[측정]');
  for (const s of steps) lines.push(`- ${s.label}: ${STEP_TEXT[s.status]}${s.detail ? ` - ${s.detail}` : ''}`);
  lines.push('', '[판정 (추정)]');
  if (llm) {
    lines.push(
      `- 생기부 평어 초안(3B급): 학생 1명 약 ${formatSeconds(llm.perStudentSec)}, ${CLASS_SIZE}명 약 ${formatSeconds(
        llm.classMinutes * 60,
      )} → ${GRADE_LABEL[llm.grade]}`,
    );
  }
  if (stt) lines.push(`- 음성 자막(${CLIP_SECONDS}초 녹음): 약 ${formatSeconds(stt.perClipSec)} → ${GRADE_LABEL[stt.grade]}`);
  if (!llm && !stt) lines.push('- 측정 결과 없음');
  return lines.join('\n') + '\n';
}
