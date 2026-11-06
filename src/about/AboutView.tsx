import { useEffect, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import notices from '../../src-tauri/resources/THIRD_PARTY_NOTICES.md?raw';
import { BUILTIN_PLAYS } from '../board/api';
import { backupApi, dbStateApi, mediaProtectionApi, type BackupCounts, type BackupStatus, type DbStateInfo, type MediaProtection } from '../backup/api';
import { curriculum } from '../curriculum';
import { formatBytes } from '../diag/estimate';
import { errorText } from '../records/api';
import activities from '../../data/activities/activities.json';
import licenses from './licenses.json';
import { CURRICULUM_SOURCE, INSTALL_WARNING_STEPS, PRIVACY_NOTE, SUPPORT_EMAIL, UPDATE_STEPS } from './content';
import Markdown from './Markdown';
import SupportExport from '../support/SupportExport';

interface AppInfo {
  version: string;
  os: string;
  arch: string;
}

interface StorageUsage {
  dataDir: string;
  revealPath: string;
  dbBytes: number;
  mediaBytes: number;
  modelsBytes: number;
  totalBytes: number;
  counts: BackupCounts;
}

const ACTIVITY_COUNT = (activities as { activities: unknown[] }).activities.length;

function when(iso: string) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('ko-KR', { dateStyle: 'long', timeStyle: 'short' });
}

/** 「기록 보호」 안의 사진·음성 한 줄. */
function MediaProtectionLine({ media, locked }: { media: MediaProtection | null; locked: boolean }) {
  if (locked) return null;
  if (media === null) return <p className="diag-note">사진·음성 상태를 확인하는 중…</p>;
  if (media.state === 'working') {
    return (
      <p className="diag-note" role="status">
        사진·음성 암호화 중 ({Math.min(media.done, media.total)}/{media.total})
      </p>
    );
  }
  if (media.plain > 0) {
    return (
      <p className="diag-note" role="status">
        사진·음성 {media.plain}개는 아직 암호화하지 못했습니다. 원본은 그대로 있으며 앱을 다시 시작하면 다시 시도합니다.
      </p>
    );
  }
  return (
    <p className="diag-note" role="status">
      사진·음성도 암호화되어 있습니다.
    </p>
  );
}

export default function AboutView({ onGoBackup }: { onGoBackup: () => void }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [storage, setStorage] = useState<StorageUsage | null>(null);
  const [status, setStatus] = useState<BackupStatus | null>(null);
  const [dbState, setDbState] = useState<DbStateInfo | null>(null);
  const [media, setMedia] = useState<MediaProtection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const [showFull, setShowFull] = useState(false);
  const [fullText, setFullText] = useState<string | null>(null);
  const [fullError, setFullError] = useState<string | null>(null);
  const tauri = isTauri();

  // 전문은 크기가 커서 처음 열 때만 불러온다(주 번들에 넣지 않음).
  useEffect(() => {
    if (!showFull || fullText !== null) return;
    import('../../src-tauri/resources/THIRD_PARTY_LICENSES.md?raw').then(
      (m) => setFullText(m.default),
      (e) => setFullError(errorText(e)),
    );
  }, [showFull, fullText]);

  useEffect(() => {
    if (!tauri) return;
    invoke<AppInfo>('app_info').then(setInfo, (e) => setError(errorText(e)));
    invoke<StorageUsage>('about_storage').then(setStorage, (e) => setError(errorText(e)));
    backupApi.status().then(setStatus, (e) => setError(errorText(e)));
    dbStateApi.get().then(setDbState, (e) => setError(errorText(e)));
  }, [tauri]);

  // 사진·음성 암호화 진행 상황: 전환 중이면 2초마다 다시 확인한다.

  useEffect(() => {
    if (!tauri) return;
    let stop = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = () =>
      mediaProtectionApi.get().then(
        (m) => {
          if (stop) return;
          setMedia(m);
          if (m.state === 'working') timer = setTimeout(tick, 2000);
        },
        () => {
          // 상태를 못 읽어도 화면의 나머지는 그대로 둔다.
        },
      );
    void tick();
    return () => {
      stop = true;
      if (timer) clearTimeout(timer);
    };
    // 처음 한 번 시작해 스스로 이어 간다.
  }, [tauri]);


  const standards = curriculum.standards;
  const count = (v: string) => standards.filter((s) => s.verification === v).length;

  return (
    <main className="diag about">
      {error && <p className="diag-error">정보를 불러오지 못했습니다: {error}</p>}

      <section className="diag-section" aria-label="앱">
        <h2>앱</h2>
        <dl className="about-list">
          <dt>이름</dt>
          <dd>바슬즐 탐험대</dd>
          <dt>버전</dt>
          <dd>{info ? `v${info.version} (${info.os}/${info.arch})` : tauri ? '불러오는 중…' : '브라우저 미리보기'}</dd>
          <dt>교육과정 데이터</dt>
          <dd>v{curriculum.framework.dataVersion}</dd>
          <dt>들어 있는 카드</dt>
          <dd>
            놀이 카드 {BUILTIN_PLAYS.length}개 · 활동 카드 {ACTIVITY_COUNT}개
          </dd>
        </dl>
      </section>

      <section className="diag-section" aria-label="내 데이터">
        <h2>내 데이터</h2>
        <p className="diag-note">{PRIVACY_NOTE}</p>
        {!tauri ? (
          <p className="diag-note">데이터 위치와 사용량은 설치된 앱에서만 보입니다.</p>
        ) : storage ? (
          <>
            <p className="diag-hint">
              데이터 폴더: <code>{storage.dataDir}</code>
            </p>
            <button
              className="diag-secondary"
              onClick={() =>
                revealItemInDir(storage.revealPath).then(
                  () => setOpenError(null),
                  (e) => setOpenError(errorText(e)),
                )
              }
            >
              폴더 열기
            </button>
            {openError && <p className="diag-error">폴더를 열 수 없습니다: {openError}</p>}

            <h3>사용량</h3>
            <dl className="about-list">
              <dt>기록(DB)</dt>
              <dd>{formatBytes(storage.dbBytes)}</dd>
              <dt>사진·음성 첨부</dt>
              <dd>{formatBytes(storage.mediaBytes)}</dd>
              <dt>내려받은 AI 모델</dt>
              <dd>{formatBytes(storage.modelsBytes)}</dd>
              <dt>합계</dt>
              <dd>
                <strong>{formatBytes(storage.totalBytes)}</strong>
              </dd>
            </dl>

            <h3>기록 개수</h3>
            <dl className="about-list">
              <dt>학급</dt>
              <dd>{storage.counts.classes}개</dd>
              <dt>학생</dt>
              <dd>{storage.counts.students}명</dd>
              <dt>관찰 기록</dt>
              <dd>{storage.counts.observations}개</dd>
              <dt>첨부</dt>
              <dd>{storage.counts.attachments}개</dd>
            </dl>
          </>
        ) : (
          <p className="diag-note">불러오는 중…</p>
        )}

        {tauri && (
          <>
            <h3>백업</h3>
            <p className="diag-note">
              {status
                ? status.lastBackup
                  ? `마지막 백업: ${when(status.lastBackup.at)} (그 뒤 관찰 기록 ${status.observationsSince}개 추가)`
                  : '아직 백업한 적이 없습니다.'
                : '불러오는 중…'}
            </p>
            <button className="diag-primary" onClick={onGoBackup}>
              백업 탭으로
            </button>
          </>
        )}
      </section>

      <section className="diag-section" aria-label="기록 보호">
        <h2>기록 보호</h2>
        {!tauri ? (
          <p className="diag-note">기록 보호 상태는 설치된 앱에서만 보입니다.</p>
        ) : dbState === null ? (
          <p className="diag-note">불러오는 중…</p>
        ) : dbState.state === 'plain' ? (
          <>
            <p className="diag-error" role="alert">
              기록 데이터베이스를 아직 암호화하지 못했습니다. 지금은 예전처럼 평문으로 저장되어 있습니다. 앱을 다시 시작하면 다시 시도합니다.
            </p>
            {dbState.reason && <p className="diag-hint">이유: {dbState.reason}</p>}
            <p className="diag-note">사진·음성 파일은 기록 데이터베이스를 암호화한 뒤에 암호화합니다.</p>
          </>
        ) : (
          <>
            <p className="diag-note">기록 데이터베이스가 이 PC의 윈도우 계정에 묶인 키로 암호화되어 있습니다.</p>
            <MediaProtectionLine media={media} locked={dbState.state === 'locked'} />
          </>
        )}
      </section>

      <section className="diag-section" aria-label="문의하기">
        <h2>문의하기</h2>
        <p className="diag-note">
          문제가 생기면 진단 파일을 만들어 보내 주세요. 앱 버전·PC 사양·기록 개수(숫자만)와 오류 기록이 들어 있으며, 학생 이름과 기록 내용은 들어가지 않습니다. 파일은 이 PC에만
          만들어지고 앱이 자동으로 보내지 않습니다.
        </p>
        <p className="diag-note">
          문의처: <strong>{SUPPORT_EMAIL}</strong>
        </p>
        <SupportExport />
      </section>

      <section className="diag-section" aria-label="업데이트 방법">
        <h2>업데이트 방법</h2>
        <ol className="about-steps">
          {UPDATE_STEPS.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
      </section>

      <section className="diag-section" aria-label="설치 경고 안내">
        <h2>설치할 때 「Windows의 PC 보호」가 뜨면</h2>
        <ul className="about-steps">
          {INSTALL_WARNING_STEPS.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ul>
      </section>

      <section className="diag-section" aria-label="오픈소스와 모델 라이선스">
        <h2>오픈소스와 모델 라이선스</h2>
        <Markdown source={notices} />
        <h3>화면(프런트엔드) 구성 요소</h3>
        <LicenseTable rows={licenses.npm} />
        <h3>프로그램(Rust) 구성 요소</h3>
        <LicenseTable rows={licenses.rust} />
        <button className="diag-secondary" onClick={() => setShowFull(true)}>
          전체 라이선스 보기
        </button>
      </section>

      <section className="diag-section" aria-label="교육과정 출처">
        <h2>교육과정 출처</h2>
        <p>{CURRICULUM_SOURCE}</p>
        <p className="diag-note">
          성취기준 {standards.length}개 중 원문 확인 {count('verified')}개 · 원문 확인 필요 {count('unverified-spec')}개 · 추정{' '}
          {count('inferred')}개. 「원문 확인 필요」「추정」으로 표시된 내용은 공식 문서와 대조해 쓰세요.
        </p>
      </section>

      {showFull && (
        <div className="license-panel" role="dialog" aria-modal="true" aria-label="전체 라이선스">
          <div className="license-panel-bar">
            <h2>전체 라이선스</h2>
            <button className="diag-primary" onClick={() => setShowFull(false)}>
              닫기
            </button>
          </div>
          {fullError ? (
            <p className="diag-error">라이선스 전문을 불러오지 못했습니다: {fullError}</p>
          ) : fullText === null ? (
            <p className="diag-note">불러오는 중…</p>
          ) : (
            <pre className="license-panel-text" tabIndex={0}>
              {fullText}
            </pre>
          )}
        </div>
      )}
    </main>
  );
}

function LicenseTable({ rows }: { rows: { name: string; version: string; license: string }[] }) {
  return (
    <div className="md-table" tabIndex={0}>
      <table>
        <thead>
          <tr>
            <th>이름</th>
            <th>버전</th>
            <th>라이선스</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.name}>
              <td>{r.name}</td>
              <td>{r.version}</td>
              <td>{r.license}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
