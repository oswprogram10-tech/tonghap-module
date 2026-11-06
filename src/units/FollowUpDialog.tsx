import { useEffect, useState } from 'react';
import { errorText } from '../records/api';
import { ActivityCardView } from './ActivityPicker';
import { activitiesApi, embedApi, questionLines, type Activity, type QuestionGroup, type Recommendation } from './activities';
import { unitsApi, type Session, type UnitDetail } from './api';

interface Props {
  unit: UnitDetail;
  session: Session;
  initialQuestions: string;
  initialReflection: string;
  onCancel: () => void;
  /** 새 차시를 만들었으면 그 차시를 함께 넘긴다. */
  onDone: (unit: UnitDetail, created: Session | null) => void;
}

/** 꼬리 잇기: 차시를 마치며 학생 질문을 적고 다음 차시를 만든다. */
export default function FollowUpDialog({ unit, session, initialQuestions, initialReflection, onCancel, onDone }: Props) {
  const [questions, setQuestions] = useState(initialQuestions);
  const [reflection, setReflection] = useState(initialReflection);
  const [nextTitle, setNextTitle] = useState('');
  const [stageId, setStageId] = useState(session.stageId);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recs, setRecs] = useState<Recommendation[] | null>(null);
  const [recError, setRecError] = useState<string | null>(null);
  const [card, setCard] = useState<Activity | null>(null);
  // 뜻이 비슷한 질문 묶기(모델이 있을 때만). 묶음은 화면에만 두고 저장하지 않는다.
  const [canGroup, setCanGroup] = useState(false);
  const [groups, setGroups] = useState<QuestionGroup[] | null>(null);
  const [groupBusy, setGroupBusy] = useState(false);
  const [groupError, setGroupError] = useState<string | null>(null);
  const [focus, setFocus] = useState<number | null>(null);

  useEffect(() => {
    let stale = false;
    embedApi
      .status()
      .then((s) => {
        if (!stale) setCanGroup(s.ready);
      })
      .catch(() => {
        if (!stale) setCanGroup(false);
      });
    return () => {
      stale = true;
    };
  }, []);

  const lines = questionLines(questions);
  // 추천에 쓰는 질문: 묶음을 눌렀으면 그 묶음의 질문들, 아니면 적은 질문 전체.
  const recQuestions = focus !== null && groups?.[focus] ? groups[focus].questions.join('\n') : questions;

  const changeQuestions = (v: string) => {
    setQuestions(v);
    // 질문을 고치면 묶음은 낡은 것이 된다.
    setGroups(null);
    setFocus(null);
    setGroupError(null);
  };

  const group = async () => {
    setGroupBusy(true);
    setGroupError(null);
    setFocus(null);
    try {
      setGroups(await embedApi.cluster(lines));
    } catch (e) {
      setGroups(null);
      setGroupError(errorText(e));
    } finally {
      setGroupBusy(false);
    }
  };

  // 학생 질문 입력이 멈추고 0.5초 뒤에 카드를 추천받는다.
  useEffect(() => {
    let stale = false;
    const timer = setTimeout(() => {
      activitiesApi
        .recommend({ unitId: unit.id, afterSessionId: session.id, questions: recQuestions })
        .then((r) => {
          if (stale) return;
          setRecs(r);
          setRecError(null);
        })
        .catch((e) => {
          if (!stale) setRecError(errorText(e));
        });
    }, 500);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [unit.id, session.id, recQuestions]);

  const pick = (a: Activity) => {
    if (card?.id === a.id) {
      setCard(null);
      return;
    }
    setCard(a);
    setNextTitle(a.title);
  };

  const run = async (withNext: boolean) => {
    setError(null);
    if (withNext && !nextTitle.trim()) {
      setError('다음 차시 제목을 입력해 주세요.');
      return;
    }
    setBusy(true);
    try {
      const before = new Set(unit.stages.flatMap((g) => g.sessions.map((s) => s.id)));
      const detail = await unitsApi.completeAndFollow(session.id, questions, reflection, withNext ? { title: nextTitle.trim(), stageId, activityId: card?.id ?? null } : undefined);
      const created = withNext ? (detail.stages.flatMap((g) => g.sessions).find((s) => !before.has(s.id)) ?? null) : null;
      onDone(detail, created);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <div className="rec-modal unit-follow-modal" role="dialog" aria-modal="true" aria-label="마치고 다음 차시 잇기">
      <div className="rec-modal-body">
        <h2>「{session.title}」 마치기</h2>
        <label className="rec-field">
          학생 질문·반응
          <textarea className="rec-textarea" rows={4} value={questions} placeholder="아이들이 궁금해한 것, 이어서 해 보고 싶다고 한 것" onChange={(e) => changeQuestions(e.target.value)} />
        </label>
        {canGroup && lines.length >= 3 && (
          <div className="rec-row rec-wrap" style={{ marginTop: 8 }}>
            <button type="button" className="diag-secondary" disabled={groupBusy} onClick={group}>
              {groupBusy ? '묶는 중…' : '비슷한 질문 묶기'}
            </button>
            <span className="diag-hint">질문을 줄마다 하나씩 적으면 뜻이 비슷한 것끼리 묶어 줍니다. 묶음은 저장되지 않습니다.</span>
          </div>
        )}
        {groupError && <p className="diag-error">{groupError}</p>}
        {groups !== null && (
          <ul className="act-list" aria-label="비슷한 질문 묶음" style={{ marginTop: 8 }}>
            {groups.map((g, i) => (
              <li key={i} className="unit-q-group" style={{ border: '2px solid var(--line)', borderRadius: 14, padding: 12, listStyle: 'none', marginBottom: 8 }}>
                <strong>
                  {g.questions.length > 1 ? `묶음 ${i + 1}` : '혼자 남은 질문'} · {g.questions.length}개
                </strong>
                <p className="diag-hint">대표 질문: {g.representative}</p>
                <ul>
                  {g.questions.map((q, k) => (
                    <li key={k}>{q}</li>
                  ))}
                </ul>
                <button type="button" className="diag-secondary" aria-pressed={focus === i} style={{ marginTop: 8 }} onClick={() => setFocus(focus === i ? null : i)}>
                  {focus === i ? '이 묶음으로 추천 중 (다시 누르면 전체)' : '이 묶음으로 추천받기'}
                </button>
              </li>
            ))}
          </ul>
        )}
        <label className="rec-field" style={{ marginTop: 8 }}>
          성찰
          <textarea className="rec-textarea" rows={3} value={reflection} onChange={(e) => setReflection(e.target.value)} />
        </label>

        <h3 className="rec-h">추천 활동 카드</h3>
        {recError && <p className="diag-error">{recError}</p>}
        {recs === null && !recError && <p className="diag-hint">추천 카드를 찾는 중…</p>}
        {recs !== null && recs.length === 0 && <p className="diag-hint">추천할 카드가 없습니다. 아래에서 직접 제목을 적어 주세요.</p>}
        {recs !== null && recs.length > 0 && (
          <ul className="act-list act-recs">
            {recs.map((r) => (
              <ActivityCardView
                key={r.activity.id}
                card={r.activity}
                reasons={r.reasons}
                selected={card?.id === r.activity.id}
                pickLabel={card?.id === r.activity.id ? '선택함 (다시 누르면 취소)' : '이 카드로 다음 차시 만들기'}
                onPick={() => pick(r.activity)}
              />
            ))}
          </ul>
        )}

        <h3 className="rec-h">다음 차시</h3>
        <div className="rec-row rec-wrap">
          <label className="rec-field rec-grow">
            다음 차시 제목
            <input value={nextTitle} maxLength={100} onChange={(e) => setNextTitle(e.target.value)} />
          </label>
          <label className="rec-field">
            단계
            <select value={stageId} onChange={(e) => setStageId(e.target.value)}>
              {unit.stages.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.position + 1}. {g.title}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="diag-hint">학생 질문이 다음 차시 활동 내용 첫머리에 들어갑니다. {card ? `활동 카드 「${card.title}」의 진행 순서·시간·성취기준도 함께 들어갑니다.` : '성취기준은 다음 차시에서 새로 골라 주세요.'}</p>
        {error && <p className="diag-error">{error}</p>}
        <div className="rec-row rec-wrap" style={{ marginTop: 16 }}>
          <button className="diag-primary" disabled={busy} onClick={() => run(true)}>
            완료하고 다음 차시 만들기
          </button>
          <button className="diag-secondary" disabled={busy} onClick={() => run(false)}>
            다음 차시 없이 마치기
          </button>
          <button className="diag-secondary" disabled={busy} onClick={onCancel}>
            취소
          </button>
        </div>
      </div>
    </div>
  );
}
