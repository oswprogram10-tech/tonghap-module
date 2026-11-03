import { useState } from 'react';
import { errorText } from '../records/api';
import { GROUP_LABEL, PLACE_LABEL, PLAY_TAGS, boardApi, type Group, type Place, type Play } from './api';

interface Draft {
  id?: string;
  name: string;
  place: Place;
  group: Group;
  minutes: number;
  rules: string;
  safety: string;
  materials: string;
  tags: string[];
  topicBookIds: string;
}

const EMPTY: Draft = { name: '', place: 'outdoor', group: 'whole', minutes: 10, rules: '', safety: '', materials: '', tags: ['신체'], topicBookIds: '' };
const lines = (s: string) => s.split('\n').map((x) => x.trim()).filter(Boolean);

const toDraft = (p: Play): Draft => ({
  ...p,
  rules: p.rules.join('\n'),
  safety: p.safety.join('\n'),
  materials: p.materials.join(', '),
  topicBookIds: p.topicBookIds.join(', '),
});

/** 교사 전용 화면이라 글자가 작아도 된다. */
export default function PlayManager({ custom, onChanged, onClose }: { custom: Play[]; onChanged: () => Promise<void>; onClose: () => void }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    if (!draft) return;
    try {
      await boardApi.customPlaySave({
        id: draft.id,
        name: draft.name,
        place: draft.place,
        group: draft.group,
        minutes: draft.minutes,
        rules: lines(draft.rules),
        safety: lines(draft.safety),
        materials: draft.materials.split(/[,\n]/).map((x) => x.trim()).filter(Boolean),
        tags: draft.tags,
        topicBookIds: draft.topicBookIds.split(/[,\s]+/).filter(Boolean),
      });
      setDraft(null);
      setError(null);
      await onChanged();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const remove = async (p: Play) => {
    if (!window.confirm(`'${p.name}' 놀이를 지울까요?`)) return;
    try {
      await boardApi.customPlayDelete(p.id);
      await onChanged();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setDraft((d) => (d ? { ...d, [k]: v } : d));

  return (
    <div className="bd-teacher">
      <div className="bd-bar">
        <h2 className="bd-h3 bd-grow">놀이 관리 (교사용)</h2>
        <button className="bd-small" onClick={onClose}>
          칠판으로 돌아가기
        </button>
      </div>
      {error && <p className="bd-error">{error}</p>}
      {!draft ? (
        <>
          <button className="bd-small" onClick={() => setDraft({ ...EMPTY })}>
            + 새 놀이
          </button>
          {custom.length === 0 && <p className="bd-t-note">직접 만든 놀이가 없습니다. 기본 놀이는 고칠 수 없습니다.</p>}
          <ul className="bd-t-list">
            {custom.map((p) => (
              <li key={p.id}>
                <span className="bd-grow">
                  {p.name} · {PLACE_LABEL[p.place]} · {p.minutes}분
                </span>
                <button className="bd-small" onClick={() => setDraft(toDraft(p))}>
                  수정
                </button>
                <button className="bd-small" onClick={() => remove(p)}>
                  삭제
                </button>
              </li>
            ))}
          </ul>
        </>
      ) : (
        <form
          className="bd-t-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label>
            이름 (20자 이하)
            <input value={draft.name} maxLength={20} onChange={(e) => set('name', e.target.value)} />
          </label>
          <div className="bd-t-row">
            <label>
              장소
              <select value={draft.place} onChange={(e) => set('place', e.target.value as Place)}>
                {Object.entries(PLACE_LABEL).map(([v, l]) => (
                  <option key={v} value={v}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
            <label>
              인원 구성
              <select value={draft.group} onChange={(e) => set('group', e.target.value as Group)}>
                {Object.entries(GROUP_LABEL).map(([v, l]) => (
                  <option key={v} value={v}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
            <label>
              시간(분, 5~40)
              <input type="number" min={5} max={40} value={draft.minutes} onChange={(e) => set('minutes', Number(e.target.value))} />
            </label>
          </div>
          <label>
            규칙 (한 줄에 하나, 2~5개)
            <textarea rows={4} value={draft.rules} onChange={(e) => set('rules', e.target.value)} />
          </label>
          <label>
            안전 수칙 (한 줄에 하나, 1~3개)
            <textarea rows={2} value={draft.safety} onChange={(e) => set('safety', e.target.value)} />
          </label>
          <label>
            준비물 (쉼표로 구분, 없으면 비움)
            <input value={draft.materials} onChange={(e) => set('materials', e.target.value)} />
          </label>
          <fieldset>
            <legend>놀이 종류</legend>
            {PLAY_TAGS.map((t) => (
              <label key={t} className="bd-t-check">
                <input
                  type="checkbox"
                  checked={draft.tags.includes(t)}
                  onChange={(e) => set('tags', e.target.checked ? [...draft.tags, t] : draft.tags.filter((x) => x !== t))}
                />
                {t}
              </label>
            ))}
          </fieldset>
          <label>
            어울리는 주제책 번호 (선택, 쉼표로 구분. 예: g1s1-korea)
            <input value={draft.topicBookIds} onChange={(e) => set('topicBookIds', e.target.value)} />
          </label>
          <div className="bd-t-row">
            <button className="bd-small" type="submit">
              저장
            </button>
            <button className="bd-small" type="button" onClick={() => setDraft(null)}>
              취소
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
