// 근거 기록을 보여 주는 조각(설계 25). 교과·행동특성 단계 화면이 함께 쓴다.
import { shorten, standardByCode } from '../records/util';
import type { Evidence } from './api';

const md = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8, 10))}`;

export const KIND_LABEL = { observation: '관찰', habit: '습관', session: '차시' } as const;

/** 짧은 이름(체크 상자·단추 이름표용). */
export function evidenceLabel(e: Evidence): string {
  if (e.kind === 'observation') return `${md(e.date)} ${shorten(e.text || e.quote || '(메모 없음)', 14)}`;
  if (e.kind === 'habit') return `습관 ${shorten(e.title, 12)}`;
  return `${md(e.date)} 차시 ${shorten(e.title, 12)}`;
}

/** 한 줄 요약: 날짜, 내용. */
export function evidenceDate(e: Evidence): string {
  return e.kind === 'habit' ? '습관' : md(e.date);
}

export function evidenceSummary(e: Evidence, n = 44): string {
  if (e.kind === 'observation') return shorten(e.text || e.quote || '(메모 없음)', n);
  if (e.kind === 'habit') return `${shorten(e.title, n)} · ${e.schoolDays}일 중 ${e.doneDays}일`;
  return shorten(e.title, n);
}

export function isNegative(e: Evidence): boolean {
  return e.kind === 'observation' && !!e.negative;
}

/** 근거 한 건의 자세한 내용(그 자리에서 펼쳐 보이는 부분). */
export function EvidenceDetail({ e, bundle }: { e: Evidence; bundle: Record<string, string> }) {
  const stds = e.kind === 'habit' ? [] : e.standards;
  return (
    <div className="ev-detail">
      {e.kind === 'observation' && (
        <>
          <p>
            {e.date} · {e.topic ?? '주제책 없음'}
            {e.together && ' · 친구와 함께 기록된 관찰'}
          </p>
          {e.text && <p className="ev-quote-text">{e.text}</p>}
          {e.quote && <p className="ev-quote-text">음성 메모 자막: “{e.quote}”</p>}
          {e.tags.length > 0 && <p>태그: {e.tags.join(', ')}</p>}
          {e.negative && <p className="rec-tag rec-tag-guidance">지도가 필요한 모습</p>}
        </>
      )}
      {e.kind === 'habit' && (
        <p>
          「{e.title}」 {e.schoolDays}일 중 {e.doneDays}일 실천
        </p>
      )}
      {e.kind === 'session' && (
        <p>
          {e.date} · {e.topic ?? '주제책 없음'} · {e.title}
        </p>
      )}
      {stds.map((c) => (
        <p key={c} className="diag-hint">
          <code>{c}</code> {bundle[c] ?? standardByCode(c)?.standard.statement ?? ''}
        </p>
      ))}
    </div>
  );
}
