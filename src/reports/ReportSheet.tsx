import { forwardRef, useLayoutEffect, useRef } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { HabitSummary } from '../board/api';
import { curriculum } from '../curriculum';
import { shorten, topicTitle } from '../records/util';
import { MAX_NOTE_CHARS, SHEET_HEIGHT, SHEET_WIDTH, type Candidate, type ReportDraft, type ReportObservation, type ReportPhoto } from './api';

export interface SheetData {
  schoolYear: number;
  grade: number;
  className: string;
  studentName: string;
  studentNumber: number;
  topicBookId: string | null;
  from: string;
  to: string;
  lessons: string[];
  observations: ReportObservation[];
  photos: ReportPhoto[];
  quotes: string[];
  habits: HabitSummary[];
  message: string;
}

export interface Selection {
  observationIds: string[];
  attachmentIds: string[];
  quoteAttachmentIds: string[];
  message: string;
}

/** 초안과 선택 상태에서 그릴 내용을 만든다. 선택 순서를 그대로 쓴다. */
export function sheetData(d: ReportDraft, sel: Selection): SheetData {
  const byObs = new Map(d.candidates.map((c) => [c.observation.id, c]));
  const photoById = new Map<string, ReportPhoto>();
  const quoteById = new Map<string, string>();
  for (const c of d.candidates as Candidate[]) {
    for (const p of c.photos) photoById.set(p.id, p);
    for (const a of c.audioWithTranscript) quoteById.set(a.id, a.transcript);
  }
  const pick = <T,>(ids: string[], m: Map<string, T>) => ids.flatMap((id) => (m.has(id) ? [m.get(id) as T] : []));
  return {
    schoolYear: d.schoolYear,
    grade: d.grade,
    className: d.className,
    studentName: d.student.name,
    studentNumber: d.student.number,
    topicBookId: d.topicBookId,
    from: d.from,
    to: d.to,
    lessons: d.lessons,
    observations: sel.observationIds.flatMap((id) => (byObs.has(id) ? [(byObs.get(id) as Candidate).observation] : [])),
    photos: pick(sel.attachmentIds, photoById),
    quotes: pick(sel.quoteAttachmentIds, quoteById),
    habits: d.habits,
    message: sel.message,
  };
}

const PAD_TOP = 56;
const PAD_X = 72;
const FOOT = 72;
const MIN_SCALE = 0.55;
/** 내용이 쓸 수 있는 높이(머리 여백과 바닥 문구 자리를 뺀 값). */
const CONTENT_HEIGHT = SHEET_HEIGHT - PAD_TOP - FOOT;

const md = (iso: string) => {
  const d = new Date(iso.length === 10 ? `${iso}T00:00:00` : iso);
  return `${d.getMonth() + 1}월 ${d.getDate()}일`;
};

/** 리포트 한 장. 화면 크기와 무관하게 항상 1240×1754px로 그린다(미리보기는 바깥에서 줄인다). */
const ReportSheet = forwardRef<HTMLDivElement, { data: SheetData }>(function ReportSheet({ data }, ref) {
  const title = topicTitle(data.topicBookId);
  const topic = curriculum.topicBooks.find((t) => t.id === data.topicBookId);
  // 완료된 차시가 없으면 주제책 소주제를 보여 준다.
  const learned = data.lessons.length > 0 ? data.lessons : (topic?.subtopics ?? []).slice(0, 8);
  const inner = useRef<HTMLDivElement>(null);
  // 고를 수 있는 내용이 많아 한 장을 넘치면, 용지 크기는 그대로 두고 내용만 줄여서 맞춘다.
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const baseWidth = SHEET_WIDTH - PAD_X * 2;
    el.style.width = `${baseWidth}px`;
    el.style.transform = '';
    const scale = Math.max(MIN_SCALE, Math.min(1, CONTENT_HEIGHT / el.scrollHeight));
    if (scale < 1) {
      el.style.width = `${baseWidth / scale}px`;
      el.style.transform = `scale(${scale})`;
    }
  }, [data]);
  return (
    <div ref={ref} className="rp-sheet" style={{ width: SHEET_WIDTH, height: SHEET_HEIGHT, padding: `${PAD_TOP}px ${PAD_X}px 0` }}>
      <div ref={inner} className="rp-inner">
      <header className="rp-head">
        <p className="rp-kicker">학교생활 리포트</p>
        <h1 className="rp-name">{data.studentName}</h1>
        <p className="rp-sub">
          {data.schoolYear}학년도 {data.grade}학년 {data.className} · {data.studentNumber}번
        </p>
        <p className="rp-period">
          {title ? `「${title}」 ` : ''}
          {md(data.from)}~{md(data.to)}
        </p>
      </header>

      <section className="rp-sec">
        <h2>이렇게 배웠어요</h2>
        {learned.length === 0 ? (
          <p className="rp-empty">이 기간에 마친 수업이 없어요.</p>
        ) : (
          <ul className="rp-lessons">
            {learned.map((t, i) => (
              <li key={i}>{t}</li>
            ))}
          </ul>
        )}
      </section>

      <section className="rp-sec">
        <h2>이런 모습을 보였어요</h2>
        {data.observations.length === 0 ? (
          <p className="rp-empty">고른 관찰이 없어요.</p>
        ) : (
          data.observations.map((o) => (
            <div key={o.id} className="rp-obs">
              <div className="rp-obs-head">
                <strong>{md(o.observedAt)}</strong>
                {o.othersCount > 0 && <span className="rp-tag">친구들과 함께</span>}
                {o.tags.map((t) => (
                  <span key={t} className="rp-tag">
                    {t}
                  </span>
                ))}
              </div>
              {o.note && <p className="rp-note">{shorten(o.note, MAX_NOTE_CHARS)}</p>}
            </div>
          ))
        )}
        {data.quotes.map((q, i) => (
          <blockquote key={i} className="rp-quote">
            “{q}”
          </blockquote>
        ))}
      </section>

      {data.photos.length > 0 && (
        <section className="rp-sec">
          <h2>학교에서 찍은 사진</h2>
          <div className={data.photos.length === 1 ? 'rp-photos rp-photos-1' : 'rp-photos'}>
            {data.photos.map((p) => (
              <img key={p.id} src={convertFileSrc(p.absPath)} alt="" />
            ))}
          </div>
        </section>
      )}

      {data.habits.length > 0 && (
        <section className="rp-sec">
          <h2>생활 습관</h2>
          <ul className="rp-habits">
            {data.habits.map((h) => (
              <li key={h.habitId}>
                <span>
                  {h.icon} {h.title}
                </span>
                <strong>
                  {h.schoolDays}일 중 {h.doneDays}일
                </strong>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="rp-sec rp-message-sec">
        <h2>선생님이 전하는 말</h2>
        {data.message.trim() ? <p className="rp-message">{data.message}</p> : <p className="rp-empty">&nbsp;</p>}
      </section>

      </div>
      <footer className="rp-foot">「바슬즐 탐험대」에서 만든 리포트입니다</footer>
    </div>
  );
});

export default ReportSheet;
