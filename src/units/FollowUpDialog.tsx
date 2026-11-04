import { useEffect, useState } from 'react';
import { errorText } from '../records/api';
import { ActivityCardView } from './ActivityPicker';
import { activitiesApi, type Activity, type Recommendation } from './activities';
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

  // 학생 질문 입력이 멈추고 0.5초 뒤에 카드를 추천받는다.
  useEffect(() => {
    let stale = false;
    const timer = setTimeout(() => {
      activitiesApi
        .recommend({ unitId: unit.id, afterSessionId: session.id, questions })
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
  }, [unit.id, session.id, questions]);

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
          <textarea className="rec-textarea" rows={4} value={questions} placeholder="아이들이 궁금해한 것, 이어서 해 보고 싶다고 한 것" onChange={(e) => setQuestions(e.target.value)} />
        </label>
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
