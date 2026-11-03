import { Fragment } from 'react';
import { curriculum } from '../curriculum';
import type { ObservationQuery, SearchMatch, Student } from './api';

export type Period = 'all' | 'week' | 'month' | 'semester' | 'custom';

export interface SearchFilters {
  /** 입력 중인 검색어(찾기를 눌러야 적용). */
  draft: string;
  /** 적용된 검색어. */
  text: string;
  studentId: string;
  topicBookId: string;
  period: Period;
  from: string;
  to: string;
  hasPhoto: boolean;
  hasAudio: boolean;
}

export const EMPTY_FILTERS: SearchFilters = {
  draft: '',
  text: '',
  studentId: '',
  topicBookId: '',
  period: 'all',
  from: '',
  to: '',
  hasPhoto: false,
  hasAudio: false,
};

const pad = (n: number) => String(n).padStart(2, '0');
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

/** 기간 선택을 날짜 범위로. 이번 주는 월요일부터, 이번 학기는 3월~8월 / 9월~다음 해 2월. */
export function periodRange(f: Pick<SearchFilters, 'period' | 'from' | 'to'>, now = new Date()): { from?: string; to?: string } {
  switch (f.period) {
    case 'week': {
      const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - ((now.getDay() + 6) % 7));
      return { from: ymd(start), to: ymd(now) };
    }
    case 'month':
      return { from: ymd(new Date(now.getFullYear(), now.getMonth(), 1)), to: ymd(now) };
    case 'semester': {
      const m = now.getMonth() + 1;
      const start = m >= 3 && m <= 8 ? new Date(now.getFullYear(), 2, 1) : new Date(m >= 9 ? now.getFullYear() : now.getFullYear() - 1, 8, 1);
      return { from: ymd(start), to: ymd(now) };
    }
    case 'custom':
      return { from: f.from || undefined, to: f.to || undefined };
    default:
      return {};
  }
}

export function filtersToQuery(classId: string, f: SearchFilters): ObservationQuery {
  return {
    classId,
    studentId: f.studentId || undefined,
    topicBookId: f.topicBookId || undefined,
    text: f.text.trim() || undefined,
    ...periodRange(f),
    hasPhoto: f.hasPhoto || undefined,
    hasAudio: f.hasAudio || undefined,
  };
}

export const isEmptyQuery = (q: ObservationQuery) =>
  !q.studentId && !q.topicBookId && !q.text && !q.from && !q.to && !q.hasPhoto && !q.hasAudio;

const FIELD_LABEL: Record<SearchMatch['field'], string> = {
  note: '메모',
  transcript: '음성 자막',
  tag: '태그',
  session: '차시',
  student: '학생',
};

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** snippet 안에서 검색어를 찾아 강조한다. */
function Highlight({ text, terms }: { text: string; terms: string[] }) {
  if (terms.length === 0) return <>{text}</>;
  const re = new RegExp(`(${terms.map(escapeRe).join('|')})`, 'gi');
  return (
    <>
      {text.split(re).map((part, i) =>
        i % 2 === 1 ? (
          <mark key={i} className="rec-mark">
            {part}
          </mark>
        ) : (
          <Fragment key={i}>{part}</Fragment>
        ),
      )}
    </>
  );
}

export const searchTerms = (text: string | undefined) =>
  (text ?? '').split(/\s+/).filter(Boolean).slice(0, 5).map((w) => [...w].slice(0, 30).join(''));

export function MatchList({ matches, text }: { matches: SearchMatch[] | undefined; text: string | undefined }) {
  if (!matches || matches.length === 0) return null;
  const terms = searchTerms(text);
  return (
    <span className="rec-matches">
      {matches.map((m, i) => (
        <span key={i} className="rec-match">
          <span className="rec-match-kind">{FIELD_LABEL[m.field]}</span>
          <span className="rec-match-text">
            <Highlight text={m.snippet} terms={terms} />
          </span>
        </span>
      ))}
    </span>
  );
}

interface Props {
  filters: SearchFilters;
  onChange: (f: SearchFilters) => void;
  students: Student[];
  count: number | null;
  /** 학생 거르기를 숨긴다(포트폴리오처럼 이미 한 학생으로 정해진 곳). */
  hideStudent?: boolean;
}

/** 「기록 찾기」 검색창과 거르기 줄. */
export default function RecordSearchBar({ filters, onChange, students, count, hideStudent }: Props) {
  const set = (patch: Partial<SearchFilters>) => onChange({ ...filters, ...patch });
  const apply = () => set({ text: filters.draft.trim(), draft: filters.draft.trim() });
  const active = JSON.stringify({ ...filters, draft: '' }) !== JSON.stringify({ ...EMPTY_FILTERS, draft: '' });
  return (
    <div className="rec-search">
      <form
        className="rec-row rec-search-box"
        role="search"
        onSubmit={(e) => {
          e.preventDefault();
          apply();
        }}
      >
        <input
          type="search"
          className="rec-grow rec-search-input"
          aria-label="기록 검색어"
          placeholder="메모·음성 자막·태그·차시·이름에서 찾기 (낱말은 띄어 써요)"
          value={filters.draft}
          onChange={(e) => set({ draft: e.target.value })}
        />
        <button type="submit" className="diag-primary">
          찾기
        </button>
        <button type="button" className="diag-secondary" onClick={() => onChange(EMPTY_FILTERS)} disabled={!active && !filters.draft}>
          지우기
        </button>
      </form>
      <div className="rec-row rec-wrap rec-search-filters">
        {!hideStudent && (
          <label className="rec-field">
            학생
            <select value={filters.studentId} onChange={(e) => set({ studentId: e.target.value })}>
              <option value="">전체</option>
              {students.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.number} {s.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="rec-field">
          주제책
          <select value={filters.topicBookId} onChange={(e) => set({ topicBookId: e.target.value })}>
            <option value="">전체</option>
            {curriculum.topicBooks.map((t) => (
              <option key={t.id} value={t.id}>
                {t.grade}학년 {t.semester}학기 · {t.title}
              </option>
            ))}
          </select>
        </label>
        <label className="rec-field">
          기간
          <select value={filters.period} onChange={(e) => set({ period: e.target.value as Period })}>
            <option value="all">전체</option>
            <option value="week">이번 주</option>
            <option value="month">이번 달</option>
            <option value="semester">이번 학기</option>
            <option value="custom">직접 입력</option>
          </select>
        </label>
        {filters.period === 'custom' && (
          <>
            <label className="rec-field">
              부터
              <input type="date" value={filters.from} max={filters.to || undefined} onChange={(e) => set({ from: e.target.value })} />
            </label>
            <label className="rec-field">
              까지
              <input type="date" value={filters.to} min={filters.from || undefined} onChange={(e) => set({ to: e.target.value })} />
            </label>
          </>
        )}
        <button type="button" className={filters.hasPhoto ? 'rec-toggle on' : 'rec-toggle'} aria-pressed={filters.hasPhoto} onClick={() => set({ hasPhoto: !filters.hasPhoto })}>
          사진 있음
        </button>
        <button type="button" className={filters.hasAudio ? 'rec-toggle on' : 'rec-toggle'} aria-pressed={filters.hasAudio} onClick={() => set({ hasAudio: !filters.hasAudio })}>
          음성 있음
        </button>
      </div>
      {count !== null && active && (
        <p className="rec-search-count" role="status">
          검색 결과 {count}건
        </p>
      )}
    </div>
  );
}
