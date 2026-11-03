// data/curriculum/schema/curriculum.schema.json 과 같은 구조를 유지할 것.

export type Verification = 'unverified-spec' | 'inferred' | 'verified';

export interface Subject {
  code: string;
  id: string;
  name: string;
  focus: string;
  teachingModel: string[];
}

export interface Area {
  number: number;
  question: string;
  coreIdeas: string[];
}

export interface TopicBook {
  id: string;
  grade: number;
  semester: 1 | 2;
  order: number;
  title: string;
  area: number;
  standardIndex: number;
  sessions: number;
  subtopics: string[];
  verification: { mapping: Verification; subtopics: Verification };
}

export interface Standard {
  code: string;
  subject: string;
  area: number;
  index: number;
  statement: string | null;
  verification: Verification;
  explanations?: string[];
  considerations?: string[];
}

export interface UnitDevelopmentTool {
  id: string;
  name: string;
  steps: string[];
}

export interface Curriculum {
  framework: {
    id: string;
    title: string;
    revision: string;
    gradeBand: number[];
    dataVersion: string;
    sources: { kind: string; note: string; url?: string }[];
    requiredModulesPerTopicBook: {
      safetySessions: number;
      playSessionsMin: number;
      makingBookSessions: number;
    };
  };
  subjects: Subject[];
  areas: Area[];
  topicBooks: TopicBook[];
  standards: Standard[];
  unitDevelopmentTools: UnitDevelopmentTool[];
}
