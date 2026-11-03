import raw from '../../data/curriculum/integrated-2022.json';
import type { Area, Curriculum, Standard, Subject, TopicBook } from './types';

export const curriculum = raw as unknown as Curriculum;

export function areaOf(topic: TopicBook): Area {
  const area = curriculum.areas.find((a) => a.number === topic.area);
  if (!area) throw new Error(`영역 ${topic.area} 없음`);
  return area;
}

/** 주제책에 대응하는 교과별 성취기준 (교과 순서대로). */
export function standardsOf(topic: TopicBook): { subject: Subject; standard: Standard }[] {
  return curriculum.subjects.map((subject) => {
    const standard = curriculum.standards.find(
      (s) => s.subject === subject.code && s.area === topic.area && s.index === topic.standardIndex,
    );
    if (!standard) throw new Error(`${topic.id}: ${subject.code} 성취기준 없음`);
    return { subject, standard };
  });
}

export function topicsOf(grade: number, semester: number): TopicBook[] {
  return curriculum.topicBooks
    .filter((t) => t.grade === grade && t.semester === semester)
    .sort((a, b) => a.order - b.order);
}

export const SEMESTERS = [
  { grade: 1, semester: 1 },
  { grade: 1, semester: 2 },
  { grade: 2, semester: 1 },
  { grade: 2, semester: 2 },
] as const;
