import { useState } from 'react';
import { curriculum } from '../curriculum';
import { errorText } from '../records/api';
import { shorten, standardByCode, topicStandards } from '../records/util';
import { unitsApi, type Session, type SessionInput, type SessionKind, type SessionStandard, type SessionStatus, type StandardMode, type UnitDetail } from './api';
import { cardPlan, type Activity } from './activities';
import FollowUpDialog from './FollowUpDialog';
import { KIND_LABEL, MODE_LABEL, STATUS_LABEL, toSessions } from './util';

interface Props {
  unit: UnitDetail;
  /** 수정할 차시. 없으면 새 차시. */
  session: Session | null;
  /** 새 차시를 채울 활동 카드(고른 경우). */
  card?: Activity | null;
  /** 새 차시를 넣을 단계(수정이면 현재 단계). */
  stageId: string;
  onClose: () => void;
  onChanged: (unit: UnitDetail) => void;
  onOpenSession: (session: Session) => void;
}

const MINUTE_PRESETS = [40, 80, 120];

/** 차시 만들기·편집 창. */
export default function SessionEditor({ unit, session, card, stageId, onClose, onChanged, onOpenSession }: Props) {
  const [title, setTitle] = useState(session?.title ?? card?.title ?? '');
  const [plan, setPlan] = useState(session?.plan ?? (card ? cardPlan(card) : ''));
  const [minutes, setMinutes] = useState(session?.minutes ?? card?.minutes ?? 40);
  const [scheduledOn, setScheduledOn] = useState(session?.scheduledOn ?? '');
  const [kind, setKind] = useState<SessionKind>(session?.kind ?? card?.kind ?? 'general');
  const [status, setStatus] = useState<SessionStatus>(session?.status ?? 'planned');
  const [stage, setStage] = useState(session?.stageId ?? stageId);
  const [standards, setStandards] = useState<SessionStandard[]>(session?.standards ?? card?.standards.map((code) => ({ code, mode: 'focus' as const })) ?? []);
  const [questions, setQuestions] = useState(session?.studentQuestions ?? '');
  const [reflection, setReflection] = useState(session?.reflection ?? '');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [following, setFollowing] = useState(false);

  const topicStd = topicStandards(unit.topicBookId);
  const topicCodes = topicStd.map((x) => x.standard.code);
  const selectedCodes = standards.map((s) => s.code);
  const extraSelected = standards.filter((s) => !topicCodes.includes(s.code));
  const addable = curriculum.standards.filter((s) => !topicCodes.includes(s.code) && !selectedCodes.includes(s.code));

  const toggleStandard = (code: string) =>
    setStandards((cur) => (cur.some((s) => s.code === code) ? cur.filter((s) => s.code !== code) : [...cur, { code, mode: 'repeat' }]));
  const setMode = (code: string, mode: StandardMode) => setStandards((cur) => cur.map((s) => (s.code === code ? { ...s, mode } : s)));

  const buildInput = (): SessionInput => ({
    unitId: unit.id,
    stageId: stage,
    title: title.trim(),
    plan,
    minutes,
    kind,
    scheduledOn: scheduledOn || null,
    status,
    studentQuestions: questions,
    reflection,
    standards,
    activityId: session ? (session.activityId ?? null) : (card?.id ?? null),
  });

  const persist = async (): Promise<Session | null> => {
    setError(null);
    if (!title.trim()) {
      setError('차시 제목을 입력해 주세요.');
      return null;
    }
    if (!Number.isInteger(minutes) || minutes <= 0) {
      setError('시간은 1분 이상으로 입력해 주세요.');
      return null;
    }
    setSaving(true);
    try {
      const saved = session ? await unitsApi.sessionUpdate(session.id, buildInput()) : await unitsApi.sessionCreate(buildInput());
      onChanged(await unitsApi.get(unit.id));
      return saved;
    } catch (e) {
      setError(errorText(e));
      return null;
    } finally {
      setSaving(false);
    }
  };

  const save = async () => {
    if (await persist()) onClose();
  };

  const startFollow = async () => {
    if (await persist()) setFollowing(true);
  };

  const remove = async () => {
    if (!session || !window.confirm('이 차시를 지울까요? 연결된 관찰 기록은 남고 차시 연결만 끊어집니다.')) return;
    try {
      await unitsApi.sessionDelete(session.id);
      onChanged(await unitsApi.get(unit.id));
      onClose();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const modeRow = (s: SessionStandard) => {
    const info = standardByCode(s.code);
    return (
      <div key={s.code} className="unit-mode-row">
        <span className="unit-mode-code">
          {info && <span className={`subject-chip subject-${info.subject.id}`}>{info.subject.name}</span>} <code>{s.code}</code>
        </span>
        <span className="rec-chips" role="group" aria-label={`${s.code} 적용 방식`}>
          {(Object.keys(MODE_LABEL) as StandardMode[]).map((m) => (
            <button key={m} className={s.mode === m ? 'rec-chip on' : 'rec-chip'} aria-pressed={s.mode === m} onClick={() => setMode(s.code, m)}>
              {MODE_LABEL[m]}
            </button>
          ))}
        </span>
      </div>
    );
  };

  return (
    <div className="rec-modal" role="dialog" aria-modal="true" aria-label={session ? '차시 편집' : '차시 추가'}>
      <div className="rec-modal-body">
        <h2>{session ? '차시 편집' : '차시 추가'}</h2>
        {!session && card && <p className="diag-hint">활동 카드 「{card.title}」로 채웠습니다. 필요한 곳을 고쳐서 저장하세요.</p>}

        <label className="rec-field">
          제목
          <input value={title} maxLength={100} onChange={(e) => setTitle(e.target.value)} />
        </label>
        <label className="rec-field" style={{ marginTop: 8 }}>
          활동 내용
          <textarea className="rec-textarea" rows={5} value={plan} onChange={(e) => setPlan(e.target.value)} />
        </label>

        <div className="rec-row rec-wrap" style={{ marginTop: 12 }}>
          <label className="rec-field">
            단계
            <select value={stage} onChange={(e) => setStage(e.target.value)}>
              {unit.stages.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.position + 1}. {g.title}
                </option>
              ))}
            </select>
          </label>
          <label className="rec-field">
            날짜
            <input type="date" value={scheduledOn} onChange={(e) => setScheduledOn(e.target.value)} />
          </label>
        </div>

        <h3 className="rec-h">시간 ({toSessions(minutes)}차시)</h3>
        <div className="rec-row rec-wrap">
          <span className="rec-chips" role="group" aria-label="시간">
            {MINUTE_PRESETS.map((m) => (
              <button key={m} className={minutes === m ? 'rec-chip on' : 'rec-chip'} aria-pressed={minutes === m} onClick={() => setMinutes(m)}>
                {m}분
              </button>
            ))}
          </span>
          <label className="rec-field">
            직접 입력(분)
            <input type="number" min={1} max={600} value={Number.isNaN(minutes) ? '' : minutes} onChange={(e) => setMinutes(e.target.value === '' ? NaN : Number(e.target.value))} />
          </label>
        </div>

        <h3 className="rec-h">종류</h3>
        <div className="rec-chips" role="group" aria-label="종류">
          {(Object.keys(KIND_LABEL) as SessionKind[]).map((k) => (
            <button key={k} className={kind === k ? 'rec-chip on' : 'rec-chip'} aria-pressed={kind === k} onClick={() => setKind(k)}>
              {KIND_LABEL[k]}
            </button>
          ))}
        </div>

        <h3 className="rec-h">상태</h3>
        <div className="rec-chips" role="group" aria-label="상태">
          {(Object.keys(STATUS_LABEL) as SessionStatus[]).map((s) => (
            <button key={s} className={status === s ? 'rec-chip on' : 'rec-chip'} aria-pressed={status === s} onClick={() => setStatus(s)}>
              {STATUS_LABEL[s]}
            </button>
          ))}
        </div>

        <h3 className="rec-h">성취기준</h3>
        {topicStd.length > 0 && (
          <div className="rec-chips rec-standards" role="group" aria-label="이 주제책의 성취기준">
            {topicStd.map(({ subject, standard }) => (
              <button
                key={standard.code}
                className={selectedCodes.includes(standard.code) ? 'rec-chip rec-chip-wide on' : 'rec-chip rec-chip-wide'}
                aria-pressed={selectedCodes.includes(standard.code)}
                onClick={() => toggleStandard(standard.code)}
              >
                <span className={`subject-chip subject-${subject.id}`}>{subject.name}</span>
                <code>{standard.code}</code>
                <span className="rec-chip-text">{shorten(standard.statement, 22)}</span>
              </button>
            ))}
          </div>
        )}
        {extraSelected.length > 0 && (
          <div className="rec-chips" role="group" aria-label="그 밖의 성취기준" style={{ marginTop: 8 }}>
            {extraSelected.map((s) => (
              <button key={s.code} className="rec-chip on" aria-label={`${s.code} 빼기`} onClick={() => toggleStandard(s.code)}>
                <code>{s.code}</code> ✕
              </button>
            ))}
          </div>
        )}
        <label className="rec-field" style={{ marginTop: 8 }}>
          그 밖의 성취기준 추가
          <select
            value=""
            onChange={(e) => {
              if (e.target.value) toggleStandard(e.target.value);
            }}
          >
            <option value="">(선택해서 추가)</option>
            {addable.map((s) => (
              <option key={s.code} value={s.code}>
                {s.code} {shorten(s.statement, 30)}
              </option>
            ))}
          </select>
        </label>
        {standards.length > 0 && (
          <div className="unit-modes">
            <p className="diag-hint">성취기준마다 적용 방식을 고르세요. 반복형: 여러 차시에 걸쳐 반복 · 집중형: 한 차시에 집중 · 발현형: 활동 중 나타난 것을 반영.</p>
            {standards.map(modeRow)}
          </div>
        )}

        <h3 className="rec-h">학생 질문·반응</h3>
        <textarea className="rec-textarea" aria-label="학생 질문·반응" rows={3} value={questions} placeholder="수업 중 나온 질문과 반응을 적어 두면 다음 차시를 정할 때 도움이 됩니다." onChange={(e) => setQuestions(e.target.value)} />
        <h3 className="rec-h">성찰</h3>
        <textarea className="rec-textarea" aria-label="성찰" rows={3} value={reflection} onChange={(e) => setReflection(e.target.value)} />

        {error && <p className="diag-error">{error}</p>}
        <div className="rec-row rec-wrap" style={{ marginTop: 16 }}>
          <button className="diag-primary rec-save" disabled={saving} onClick={save}>
            {saving ? '저장 중…' : '저장'}
          </button>
          {session && session.status === 'planned' && (
            <button className="diag-secondary" disabled={saving} onClick={startFollow}>
              마치고 다음 차시 잇기
            </button>
          )}
          {session && (
            <button className="diag-secondary" onClick={remove}>
              삭제
            </button>
          )}
          <button className="diag-secondary" onClick={onClose}>
            닫기
          </button>
        </div>
      </div>

      {following && session && (
        <FollowUpDialog
          unit={unit}
          session={session}
          initialQuestions={questions}
          initialReflection={reflection}
          onCancel={() => setFollowing(false)}
          onDone={(detail, created) => {
            onChanged(detail);
            if (created) onOpenSession(created);
            else onClose();
          }}
        />
      )}
    </div>
  );
}
