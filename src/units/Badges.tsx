import type { Totals } from './api';
import { requirements } from './util';

/** 필수 편성 배지 3개: 채우면 초록, 모자라면 노랑. */
export default function RequirementBadges({ totals }: { totals: Totals }) {
  return (
    <span className="unit-badges">
      {requirements(totals).map((r) => (
        <span key={r.kind} className={r.met ? 'unit-badge met' : 'unit-badge short'} title={r.atLeast ? `${r.label} ${r.need}차시 이상` : `${r.label} ${r.need}차시`}>
          {r.label} {r.have}/{r.need}
          {r.atLeast ? '+' : ''}
        </span>
      ))}
    </span>
  );
}
