import { SEMESTERS, curriculum, standardsOf, topicsOf } from '../curriculum';
import type { Standard, Subject, TopicBook } from '../curriculum/types';

const pad = (n: number) => String(n).padStart(2, '0');

/** 로컬 시간 ISO 8601 문자열(`2027-03-05T10:20:00+09:00`). */
export function toLocalIso(d: Date): string {
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? '+' : '-';
  const abs = Math.abs(off);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}` +
    `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
  );
}

/** `<input type="datetime-local">` 값(`YYYY-MM-DDTHH:mm`). */
export function toInputValue(iso: string | Date): string {
  const d = typeof iso === 'string' ? new Date(iso) : iso;
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function fromInputValue(v: string): string {
  return toLocalIso(new Date(v));
}

export function formatDate(iso: string | null): string {
  if (!iso) return '—';
  const d = new Date(iso);
  return `${d.getFullYear()}.${d.getMonth() + 1}.${d.getDate()}`;
}

export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 학년의 주제책(1·2학기). */
export function topicsOfGrade(grade: number): TopicBook[] {
  return SEMESTERS.filter((s) => s.grade === grade).flatMap((s) => topicsOf(s.grade, s.semester));
}

export function topicTitle(id: string | null | undefined): string {
  if (!id) return '';
  return curriculum.topicBooks.find((t) => t.id === id)?.title ?? id;
}

export function topicStandards(topicId: string | null | undefined): { subject: Subject; standard: Standard }[] {
  const t = curriculum.topicBooks.find((x) => x.id === topicId);
  return t ? standardsOf(t) : [];
}

export function standardByCode(code: string): { subject: Subject; standard: Standard } | undefined {
  const standard = curriculum.standards.find((s) => s.code === code);
  const subject = standard && curriculum.subjects.find((s) => s.code === standard.subject);
  return standard && subject ? { subject, standard } : undefined;
}

export function shorten(text: string | null | undefined, n: number): string {
  if (!text) return '';
  return text.length > n ? `${text.slice(0, n)}…` : text;
}

export interface RosterLine {
  number: number | null;
  name: string;
}

/** 명단 붙여넣기 미리보기용 해석. Rust `student_import`와 같은 규칙이다(최종 판정은 Rust). */
export function parseRoster(text: string): { lines: RosterLine[]; skipped: string[] } {
  const lines: RosterLine[] = [];
  const skipped: string[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const m = /^(\d{1,4})\s*[.\t ]\s*(.+)$/.exec(line);
    if (m) {
      const number = Number(m[1]);
      const name = m[2].trim();
      if (number < 1 || number > 200 || [...name].length > 20) skipped.push(line);
      else lines.push({ number, name });
    } else if (/^\d/.test(line) || [...line].length > 20) {
      skipped.push(line);
    } else {
      lines.push({ number: null, name: line });
    }
  }
  return { lines, skipped };
}

export function currentSchoolYear(): number {
  const d = new Date();
  return d.getMonth() + 1 >= 3 ? d.getFullYear() : d.getFullYear() - 1;
}
