import { useMemo, useState } from 'react';
import { errorText, type ClassInfo } from '../records/api';
import { SEMESTERS, curriculum, topicsOf } from '../curriculum';
import { unitsApi, type UnitDetail } from './api';
import TeachingModels from './ToolReference';

const FREE_STAGE = '수업';

interface Props {
  cls: ClassInfo;
  /** 주제책 목록 화면에서 고른 주제책 등 미리 고른 값(없으면 선택 안 함). */
  initialTopicId: string | null;
  onCreated: (unit: UnitDetail) => void;
  onCancel: () => void;
}

/** 새 단원 만들기: 주제책 → 도구 → 제목. */
export default function NewUnit({ cls, initialTopicId, onCreated, onCancel }: Props) {
  const semesters = useMemo(() => SEMESTERS.filter((s) => s.grade === cls.grade), [cls.grade]);
  const topics = useMemo(() => semesters.flatMap((s) => topicsOf(s.grade, s.semester)), [semesters]);
  const [topicId, setTopicId] = useState<string | null>(initialTopicId && topics.some((t) => t.id === initialTopicId) ? initialTopicId : null);
  const [toolId, setToolId] = useState<string>(curriculum.unitDevelopmentTools[0].id);
  const [title, setTitle] = useState('');
  const [titleTouched, setTitleTouched] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const topic = topics.find((t) => t.id === topicId);
  const shownTitle = titleTouched ? title : (topic?.title ?? '');
  const tool = curriculum.unitDevelopmentTools.find((t) => t.id === toolId);

  const create = async () => {
    setError(null);
    if (!shownTitle.trim()) {
      setError('단원 제목을 입력해 주세요.');
      return;
    }
    setSaving(true);
    try {
      const unit = await unitsApi.create({
        classId: cls.id,
        topicBookId: topicId,
        toolId,
        title: shownTitle.trim(),
        stageTitles: tool ? tool.steps : [FREE_STAGE],
      });
      onCreated(unit);
    } catch (e) {
      setError(errorText(e));
      setSaving(false);
    }
  };

  return (
    <>
      <section className="diag-section">
        <div className="rec-row rec-wrap">
          <h2 className="rec-grow">새 단원 만들기</h2>
          <button className="diag-secondary" onClick={onCancel}>
            목록으로
          </button>
        </div>

        <h3 className="rec-h">1. 주제책</h3>
        <div className="unit-topic-groups" role="radiogroup" aria-label="주제책 선택">
          <button className={topicId === null ? 'rec-chip on' : 'rec-chip'} role="radio" aria-checked={topicId === null} onClick={() => setTopicId(null)}>
            주제책 없음
          </button>
          {semesters.map((s) => (
            <div key={s.semester} className="unit-topic-group">
              <strong>{s.semester}학기</strong>
              <div className="rec-chips">
                {topicsOf(s.grade, s.semester).map((t) => (
                  <button key={t.id} className={topicId === t.id ? 'rec-chip on' : 'rec-chip'} role="radio" aria-checked={topicId === t.id} onClick={() => setTopicId(t.id)}>
                    {t.order}. {t.title}
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>

        <h3 className="rec-h">2. 단원 개발 도구</h3>
        <div className="tool-grid" role="radiogroup" aria-label="단원 개발 도구 선택">
          {curriculum.unitDevelopmentTools.map((t) => (
            <button key={t.id} className={toolId === t.id ? 'tool-card tool-pick on' : 'tool-card tool-pick'} role="radio" aria-checked={toolId === t.id} onClick={() => setToolId(t.id)}>
              <h3>{t.name}</h3>
              <ol>
                {t.steps.map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ol>
            </button>
          ))}
          <button className={toolId === 'free' ? 'tool-card tool-pick on' : 'tool-card tool-pick'} role="radio" aria-checked={toolId === 'free'} onClick={() => setToolId('free')}>
            <h3>자유형</h3>
            <p className="diag-hint">단계 하나(「{FREE_STAGE}」)로 시작합니다.</p>
          </button>
        </div>

        <h3 className="rec-h">3. 제목</h3>
        <label className="rec-field">
          단원 제목
          <input
            value={shownTitle}
            maxLength={100}
            onChange={(e) => {
              setTitle(e.target.value);
              setTitleTouched(true);
            }}
          />
        </label>
        {error && <p className="diag-error">{error}</p>}
        <div className="rec-row rec-wrap" style={{ marginTop: 16 }}>
          <button className="diag-primary" disabled={saving} onClick={create}>
            {saving ? '만드는 중…' : '만들기'}
          </button>
          <button className="diag-secondary" onClick={onCancel}>
            취소
          </button>
        </div>
      </section>
      <section className="diag-section">
        <TeachingModels />
      </section>
    </>
  );
}
