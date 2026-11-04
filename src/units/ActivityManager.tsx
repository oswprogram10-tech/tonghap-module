import { useCallback, useEffect, useState } from 'react';
import { curriculum } from '../curriculum';
import { errorText, type ClassInfo } from '../records/api';
import { topicStandards, topicsOfGrade } from '../records/util';
import { ACTIVITY_KIND_LABEL, BUILTIN_ACTIVITIES, activitiesApi, subjectChip, type Activity, type ActivityKind } from './activities';
import { TEMPLATE_HELP, TEMPLATE_LABEL, TEMPLATE_PAGES, type MakingbookTemplate } from '../print/layouts';
import { ActivityCardView } from './ActivityPicker';

interface Draft {
  id?: string;
  title: string;
  topicBookId: string;
  subject: Activity['subject'];
  /** 교과 성취기준(첫째)을 뺀 나머지 선택. */
  extraStandards: string[];
  kind: ActivityKind;
  minutes: 40 | 80;
  steps: string;
  materials: string;
  keywords: string;
  /** 메이킹북 틀(빈 문자열이면 틀 없음). */
  mbTemplate: MakingbookTemplate | '';
  mbTitle: string;
  mbPages: string[];
}

const lines = (s: string) => s.split('\n').map((x) => x.trim()).filter(Boolean);
const commas = (s: string) => s.split(/[,\n]/).map((x) => x.trim()).filter(Boolean);

function emptyDraft(topicBookId: string): Draft {
  return { title: '', topicBookId, subject: '슬', extraStandards: [], kind: 'general', minutes: 40, steps: '', materials: '', keywords: '', mbTemplate: '', mbTitle: '', mbPages: [] };
}

function toDraft(a: Activity, keepId: boolean): Draft {
  return {
    id: keepId ? a.id : undefined,
    title: keepId ? a.title : `${a.title} (복사)`.slice(0, 30),
    topicBookId: a.topicBookId,
    subject: a.subject,
    extraStandards: a.standards.slice(1),
    kind: a.kind,
    minutes: a.minutes,
    steps: a.steps.join('\n'),
    materials: a.materials.join(', '),
    keywords: a.keywords.join(', '),
    mbTemplate: a.makingbook?.template ?? '',
    mbTitle: a.makingbook?.title ?? '',
    mbPages: a.makingbook?.pages ?? [],
  };
}

const ownCode = (topicBookId: string, subject: string) => topicStandards(topicBookId).find((x) => x.subject.code === subject)?.standard.code ?? '';

/** 교사 활동 카드 관리: 새로 만들기, 기본 카드 복사해 고치기, 삭제. */
export default function ActivityManager({ cls, onClose }: { cls: ClassInfo; onClose: () => void }) {
  const topics = topicsOfGrade(cls.grade);
  const [topicId, setTopicId] = useState(topics[0]?.id ?? '');
  const [custom, setCustom] = useState<Activity[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setCustom(await activitiesApi.customList());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  useEffect(() => {
    void reload();
  }, [reload]);

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setDraft((d) => (d ? { ...d, [k]: v } : d));

  const save = async () => {
    if (!draft) return;
    const first = ownCode(draft.topicBookId, draft.subject);
    try {
      await activitiesApi.customSave({
        id: draft.id,
        topicBookId: draft.topicBookId,
        title: draft.title,
        subject: draft.subject,
        standards: [first, ...draft.extraStandards.filter((c) => c !== first)],
        kind: draft.kind,
        minutes: draft.minutes,
        steps: lines(draft.steps),
        materials: commas(draft.materials),
        keywords: commas(draft.keywords),
        makingbook:
          draft.kind === 'makingbook' && draft.mbTemplate
            ? { template: draft.mbTemplate, title: draft.mbTitle.trim(), pages: draft.mbPages.slice(0, TEMPLATE_PAGES[draft.mbTemplate]).map((p) => p.trim()) }
            : undefined,
      });
      setDraft(null);
      setError(null);
      await reload();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const remove = async (a: Activity) => {
    if (!window.confirm(`'${a.title}' 카드를 지울까요? 이미 만든 차시는 그대로 남습니다.`)) return;
    try {
      await activitiesApi.customDelete(a.id);
      await reload();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const shown = [...BUILTIN_ACTIVITIES, ...custom].filter((a) => a.topicBookId === topicId);
  const std = draft ? topicStandards(draft.topicBookId) : [];

  return (
    <section className="diag-section act-manager">
      <div className="rec-row rec-wrap">
        <h2 className="rec-grow">활동 카드 관리</h2>
        <button className="diag-secondary" onClick={onClose}>
          단원 목록으로
        </button>
      </div>
      {error && <p className="diag-error">{error}</p>}
      {!draft ? (
        <>
          <div className="rec-row rec-wrap">
            <label className="rec-field">
              주제책
              <select value={topicId} onChange={(e) => setTopicId(e.target.value)}>
                {topics.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.semester}학기 {t.order}. {t.title}
                  </option>
                ))}
              </select>
            </label>
            <button className="diag-primary" onClick={() => setDraft(emptyDraft(topicId))}>
              새 카드 만들기
            </button>
          </div>
          <p className="diag-hint">기본 카드는 고칠 수 없습니다. 「복사해 고치기」로 내 카드를 만드세요.</p>
          {shown.length === 0 && <p className="diag-hint">이 주제책의 카드가 없습니다.</p>}
          <ul className="act-list">
            {shown.map((a) => {
              const mine = a.id.startsWith('custom-');
              return (
                <ActivityCardView
                  key={a.id}
                  card={a}
                  pickLabel={mine ? '수정' : '복사해 고치기'}
                  onPick={() => setDraft(toDraft(a, mine))}
                  extra={
                    mine ? (
                      <button className="diag-secondary" onClick={() => remove(a)} aria-label={`${a.title} 삭제`}>
                        삭제
                      </button>
                    ) : undefined
                  }
                />
              );
            })}
          </ul>
        </>
      ) : (
        <form
          className="act-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label className="rec-field">
            제목 (4~30자)
            <input value={draft.title} maxLength={30} onChange={(e) => set('title', e.target.value)} />
          </label>
          <div className="rec-row rec-wrap">
            <label className="rec-field">
              주제책
              <select
                value={draft.topicBookId}
                onChange={(e) => setDraft({ ...draft, topicBookId: e.target.value, extraStandards: [] })}
              >
                {topics.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.semester}학기 {t.order}. {t.title}
                  </option>
                ))}
              </select>
            </label>
            <label className="rec-field">
              주로 다루는 교과
              <select value={draft.subject} onChange={(e) => setDraft({ ...draft, subject: e.target.value as Activity['subject'], extraStandards: [] })}>
                {curriculum.subjects.map((s) => (
                  <option key={s.code} value={s.code}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <h3 className="rec-h">성취기준 (첫째는 교과의 성취기준, 더 고를 수 있어요)</h3>
          <div className="rec-chips" role="group" aria-label="성취기준">
            {std.map(({ subject, standard }) => {
              const first = subject.code === draft.subject;
              const on = first || draft.extraStandards.includes(standard.code);
              return (
                <button
                  type="button"
                  key={standard.code}
                  className={on ? 'rec-chip on' : 'rec-chip'}
                  aria-pressed={on}
                  disabled={first}
                  onClick={() =>
                    set('extraStandards', on ? draft.extraStandards.filter((c) => c !== standard.code) : [...draft.extraStandards, standard.code])
                  }
                >
                  <span className={`subject-chip subject-${subjectChip(subject.code).id}`}>{subject.name}</span>
                  <code>{standard.code}</code>
                </button>
              );
            })}
          </div>
          <h3 className="rec-h">종류와 시간</h3>
          <div className="rec-row rec-wrap">
            <span className="rec-chips" role="group" aria-label="종류">
              {(Object.keys(ACTIVITY_KIND_LABEL) as ActivityKind[]).map((k) => (
                <button type="button" key={k} className={draft.kind === k ? 'rec-chip on' : 'rec-chip'} aria-pressed={draft.kind === k} onClick={() => set('kind', k)}>
                  {ACTIVITY_KIND_LABEL[k]}
                </button>
              ))}
            </span>
            <span className="rec-chips" role="group" aria-label="시간">
              {([40, 80] as const).map((m) => (
                <button type="button" key={m} className={draft.minutes === m ? 'rec-chip on' : 'rec-chip'} aria-pressed={draft.minutes === m} onClick={() => set('minutes', m)}>
                  {m}분
                </button>
              ))}
            </span>
          </div>
          {draft.kind === 'makingbook' && (
            <>
              <h3 className="rec-h">인쇄 틀 (선택)</h3>
              <span className="rec-chips" role="group" aria-label="인쇄 틀">
                <button type="button" className={draft.mbTemplate === '' ? 'rec-chip on' : 'rec-chip'} aria-pressed={draft.mbTemplate === ''} onClick={() => set('mbTemplate', '')}>
                  틀 없음
                </button>
                {(Object.keys(TEMPLATE_LABEL) as MakingbookTemplate[]).map((t) => (
                  <button
                    type="button"
                    key={t}
                    className={draft.mbTemplate === t ? 'rec-chip on' : 'rec-chip'}
                    aria-pressed={draft.mbTemplate === t}
                    onClick={() => setDraft({ ...draft, mbTemplate: t, mbPages: Array.from({ length: TEMPLATE_PAGES[t] }, (_, i) => draft.mbPages[i] ?? '') })}
                  >
                    {TEMPLATE_LABEL[t]}
                  </button>
                ))}
              </span>
              {draft.mbTemplate && (
                <>
                  <label className="rec-field" style={{ marginTop: 8 }}>
                    표지 제목 (2~16자)
                    <input value={draft.mbTitle} maxLength={16} onChange={(e) => set('mbTitle', e.target.value)} />
                  </label>
                  {draft.mbPages.map((text, i) => (
                    <label className="rec-field" key={i} style={{ marginTop: 8 }}>
                      {draft.mbTemplate === 'zine8' ? `속 ${i + 1}쪽 안내` : TEMPLATE_HELP[draft.mbTemplate as MakingbookTemplate][i]} (2~24자)
                      <input value={text} maxLength={24} onChange={(e) => set('mbPages', draft.mbPages.map((p, j) => (j === i ? e.target.value : p)))} />
                    </label>
                  ))}
                </>
              )}
            </>
          )}
          <label className="rec-field" style={{ marginTop: 8 }}>
            진행 순서 (한 줄에 하나, 3~5줄, 한 줄 6~60자)
            <textarea className="rec-textarea" rows={5} value={draft.steps} onChange={(e) => set('steps', e.target.value)} />
          </label>
          <label className="rec-field" style={{ marginTop: 8 }}>
            준비물 (쉼표로 구분, 없으면 비움)
            <input value={draft.materials} onChange={(e) => set('materials', e.target.value)} />
          </label>
          <label className="rec-field" style={{ marginTop: 8 }}>
            추천에 쓸 낱말 (쉼표로 구분, 3~8개. 학생 질문과 맞춰 봅니다)
            <input value={draft.keywords} onChange={(e) => set('keywords', e.target.value)} />
          </label>
          <div className="rec-row rec-wrap" style={{ marginTop: 16 }}>
            <button className="diag-primary" type="submit">
              저장
            </button>
            <button className="diag-secondary" type="button" onClick={() => setDraft(null)}>
              취소
            </button>
          </div>
        </form>
      )}
    </section>
  );
}
