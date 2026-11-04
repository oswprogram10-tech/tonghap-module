import { useEffect, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { errorText } from './records/api';
import { openBoardWindow } from './board/window';
import { SEMESTERS, areaOf, curriculum, standardsOf, topicsOf } from './curriculum';
import type { TopicBook, Verification } from './curriculum/types';
import BackupView from './backup/BackupView';
import DiagView from './diag/DiagView';
import EvalView from './evaluation/EvalView';
import PortfolioView from './records/PortfolioView';
import RecordView from './records/RecordView';
import UnitsView from './units/UnitsView';

interface AppInfo {
  version: string;
  os: string;
  arch: string;
}

type View = 'record' | 'portfolio' | 'eval' | 'units' | 'topics' | 'diag' | 'backup';

const VERIFICATION_LABEL: Record<Verification, string> = {
  'unverified-spec': '원문 확인 필요',
  inferred: '추정',
  verified: '확인됨',
};

export default function App() {
  const [view, setView] = useState<View>('record');
  const [semesterIdx, setSemesterIdx] = useState(0);
  const { grade, semester } = SEMESTERS[semesterIdx];
  const topics = topicsOf(grade, semester);
  const [topicId, setTopicId] = useState(topics[0].id);
  const topic = topics.find((t) => t.id === topicId) ?? topics[0];
  const appInfo = useAppInfo();
  const [boardError, setBoardError] = useState<string | null>(null);
  // 복원 후 학급·기록을 다시 불러오도록 화면을 새로 만든다.
  const [dataKey, setDataKey] = useState(0);

  const selectSemester = (idx: number) => {
    setSemesterIdx(idx);
    const { grade: g, semester: s } = SEMESTERS[idx];
    setTopicId(topicsOf(g, s)[0].id);
  };

  return (
    <div className="app">
      <header className="app-header">
        <h1>바슬즐 탐험대</h1>
        <nav className="tabs" aria-label="화면 선택">
          <button className={view === 'record' ? 'tab active' : 'tab'} onClick={() => setView('record')}>
            관찰 기록
          </button>
          <button className={view === 'portfolio' ? 'tab active' : 'tab'} onClick={() => setView('portfolio')}>
            포트폴리오
          </button>
          <button className={view === 'eval' ? 'tab active' : 'tab'} onClick={() => setView('eval')}>
            평어
          </button>
          <button className={view === 'units' ? 'tab active' : 'tab'} onClick={() => setView('units')}>
            단원
          </button>
          <button className={view === 'topics' ? 'tab active' : 'tab'} onClick={() => setView('topics')}>
            주제책
          </button>
          <button className={view === 'diag' ? 'tab active' : 'tab'} onClick={() => setView('diag')}>
            기기 점검
          </button>
          <button className={view === 'backup' ? 'tab active' : 'tab'} onClick={() => setView('backup')}>
            백업
          </button>
        </nav>
        {isTauri() && (
          <button
            className="board-open"
            onClick={() =>
              openBoardWindow().then(
                () => setBoardError(null),
                (e) => setBoardError(errorText(e)),
              )
            }
          >
            칠판 모드
          </button>
        )}
      </header>
      {boardError && <p className="diag-error">칠판 창을 열 수 없습니다: {boardError}</p>}

      {view === 'record' ? (
        <RecordView key={dataKey} onGoBackup={() => setView('backup')} />
      ) : view === 'portfolio' ? (
        <PortfolioView key={dataKey} />
      ) : view === 'eval' ? (
        <EvalView key={dataKey} />
      ) : view === 'units' ? (
        <UnitsView key={dataKey} />
      ) : view === 'topics' ? (
        <main className="topics-layout">
          <aside className="semesters" aria-label="학기 선택">
            {SEMESTERS.map((s, idx) => (
              <button
                key={`${s.grade}-${s.semester}`}
                className={idx === semesterIdx ? 'semester active' : 'semester'}
                onClick={() => selectSemester(idx)}
              >
                {s.grade}학년 {s.semester}학기
              </button>
            ))}
          </aside>
          <section className="topic-list" aria-label="주제책">
            {topics.map((t) => (
              <button
                key={t.id}
                className={t.id === topic.id ? 'topic-card active' : 'topic-card'}
                onClick={() => setTopicId(t.id)}
              >
                <span className="topic-order">{t.order}</span>
                <span className="topic-title">{t.title}</span>
                <span className="topic-meta">{t.sessions}차시</span>
              </button>
            ))}
          </section>
          <TopicDetail topic={topic} />
        </main>
      ) : view === 'backup' ? (
        <BackupView onRestored={() => setDataKey((k) => k + 1)} />
      ) : (
        <DiagView />
      )}

      <footer className="app-footer">
        <span>
          교육과정 데이터 v{curriculum.framework.dataVersion} · 성취기준 {curriculum.standards.length}개 중 원문 확인{' '}
          {curriculum.standards.filter((s) => s.verification === 'verified').length}개
        </span>
        <span>{appInfo ? `앱 v${appInfo.version} · ${appInfo.os}/${appInfo.arch}` : '브라우저 미리보기'}</span>
      </footer>
    </div>
  );
}

function TopicDetail({ topic }: { topic: TopicBook }) {
  const area = areaOf(topic);
  const required = curriculum.framework.requiredModulesPerTopicBook;
  return (
    <article className="topic-detail">
      <p className="area-question">
        {area.number}영역 · {area.question}
      </p>
      <h2>{topic.title}</h2>
      <p className="core-idea">{area.coreIdeas.join(' ')}</p>

      <h3>성취기준</h3>
      <ul className="standards">
        {standardsOf(topic).map(({ subject, standard }) => (
          <li key={standard.code}>
            <span className={`subject-chip subject-${subject.id}`}>{subject.name}</span>
            <code>{standard.code}</code>
            <span className="statement">{standard.statement ?? '—'}</span>
            <Badge v={standard.verification} />
            <Notes label="해설" items={standard.explanations} />
            <Notes label="적용 시 고려 사항" items={standard.considerations} />
          </li>
        ))}
      </ul>

      <h3>
        소주제 <Badge v={topic.verification.subtopics} />
      </h3>
      <ol className="subtopics">
        {topic.subtopics.map((s) => (
          <li key={s}>{s}</li>
        ))}
      </ol>

      <h3>필수 편성</h3>
      <p>
        안전 더하기 {required.safetySessions}차시 · 놀이 {required.playSessionsMin}차시 이상 · 메이킹북{' '}
        {required.makingBookSessions}차시
      </p>
    </article>
  );
}

function Notes({ label, items }: { label: string; items?: string[] }) {
  if (!items || items.length === 0) return null;
  return (
    <details className="standard-notes">
      <summary>{label}</summary>
      <ul>
        {items.map((t) => (
          <li key={t}>{t}</li>
        ))}
      </ul>
    </details>
  );
}

function Badge({ v }: { v: Verification }) {
  return <span className={`badge badge-${v}`}>{VERIFICATION_LABEL[v]}</span>;
}

function useAppInfo(): AppInfo | null {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    invoke<AppInfo>('app_info')
      .then(setInfo)
      .catch((e) => console.error('app_info 호출 실패', e));
  }, []);
  return info;
}
