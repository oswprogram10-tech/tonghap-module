import { useEffect, useState } from 'react';
import { errorText } from '../records/api';
import { topicTitle } from '../records/util';
import { ACTIVITY_KIND_LABEL, BUILTIN_ACTIVITIES, activitiesApi, cardsForUnit, subjectChip, type Activity, type ActivityKind } from './activities';
import { formatMinutes } from './util';

type Filter = 'all' | ActivityKind;
const FILTERS: Filter[] = ['all', 'general', 'safety', 'makingbook'];

/** 카드 한 장의 미리보기. */
export function ActivityCardView({
  card,
  reasons,
  onPick,
  pickLabel,
  selected,
  extra,
}: {
  card: Activity;
  reasons?: string[];
  onPick: () => void;
  pickLabel?: string;
  selected?: boolean;
  /** 고르기 단추 옆에 더 둘 단추. */
  extra?: React.ReactNode;
}) {
  const subj = subjectChip(card.subject);
  return (
    <li className={selected ? 'act-card selected' : 'act-card'}>
      <div className="act-card-head">
        <strong className="act-card-title">{card.title}</strong>
        <span className={`subject-chip subject-${subj.id}`}>{subj.name}</span>
        <span className={`unit-kind kind-${card.kind}`}>{ACTIVITY_KIND_LABEL[card.kind]}</span>
        <span className="act-card-min">{formatMinutes(card.minutes)}</span>
        {card.id.startsWith('custom-') && <span className="rec-tag">내 카드</span>}
      </div>
      <div className="act-card-chips">
        {card.standards.map((c) => (
          <code key={c} className="rec-code">
            {c}
          </code>
        ))}
      </div>
      <ol className="act-card-steps">
        {card.steps.map((s, i) => (
          <li key={i}>{s}</li>
        ))}
      </ol>
      {reasons && reasons.length > 0 && (
        <ul className="act-reasons" aria-label="추천 이유">
          {reasons.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      )}
      <button className={selected ? 'diag-primary' : 'diag-secondary'} aria-pressed={selected} onClick={onPick} aria-label={`${card.title} ${pickLabel ?? '이 카드로 차시 만들기'}`}>
        {pickLabel ?? '이 카드로 차시 만들기'}
      </button>
      {extra}
    </li>
  );
}

interface Props {
  topicBookId: string | null;
  grade: number;
  onPick: (card: Activity) => void;
  onClose: () => void;
}

/** 단원 주제책의 활동 카드 목록(기본 + 교사 카드). 종류로 거를 수 있다. */
export default function ActivityPicker({ topicBookId, grade, onPick, onClose }: Props) {
  const [custom, setCustom] = useState<Activity[]>([]);
  const [filter, setFilter] = useState<Filter>('all');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    activitiesApi.customList().then(setCustom).catch((e) => setError(errorText(e)));
  }, []);

  const cards = cardsForUnit([...BUILTIN_ACTIVITIES, ...custom], topicBookId, grade).filter((a) => filter === 'all' || a.kind === filter);

  return (
    <div className="rec-modal unit-follow-modal" role="dialog" aria-modal="true" aria-label="활동 카드에서 고르기">
      <div className="rec-modal-body">
        <h2>활동 카드에서 고르기{topicBookId ? ` · ${topicTitle(topicBookId)}` : ''}</h2>
        <div className="rec-chips" role="group" aria-label="카드 종류">
          {FILTERS.map((f) => (
            <button key={f} className={filter === f ? 'rec-chip on' : 'rec-chip'} aria-pressed={filter === f} onClick={() => setFilter(f)}>
              {f === 'all' ? '전체' : ACTIVITY_KIND_LABEL[f]}
            </button>
          ))}
        </div>
        {error && <p className="diag-error">{error}</p>}
        {cards.length === 0 && <p className="diag-hint">이 조건에 맞는 카드가 없습니다.</p>}
        <ul className="act-list">
          {cards.map((c) => (
            <ActivityCardView key={c.id} card={c} onPick={() => onPick(c)} />
          ))}
        </ul>
        <div className="rec-row rec-wrap" style={{ marginTop: 16 }}>
          <button className="diag-secondary" onClick={onClose}>
            닫기
          </button>
        </div>
      </div>
    </div>
  );
}
