import { useEffect, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { ACTIVITY_TAGS, errorText, recordsApi, type Attachment, type ClassInfo, type Observation, type Student } from './api';
import AttachmentView from './Attachments';
import AudioRecorder, { type Recording } from './AudioRecorder';
import { fromInputValue, shorten, toInputValue, topicStandards, topicsOfGrade, standardByCode } from './util';

interface Props {
  cls: ClassInfo;
  students: Student[];
  /** 주제책 선택 상태(빠른 기록에서는 설정에 저장). 수정 창에서는 기록의 값. */
  topicId: string | null;
  onTopicChange: (id: string | null) => void;
  /** 수정 모드이면 기존 기록 */
  editing?: Observation;
  /** 저장 후 호출. 빠른 기록에서는 목록 새로고침. */
  onSaved: (message: string, keepOpen: boolean) => void;
  onDeleted?: () => void;
  onCancel?: () => void;
}

const basename = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** 관찰 기록 양식(빠른 기록과 수정 창이 함께 쓴다). */
export default function ObservationForm({ cls, students, topicId, onTopicChange, editing, onSaved, onDeleted, onCancel }: Props) {
  const [observedAt, setObservedAt] = useState(() => toInputValue(editing ? editing.observedAt : new Date()));
  const [selected, setSelected] = useState<string[]>(editing?.studentIds ?? []);
  const [codes, setCodes] = useState<string[]>(editing?.standardCodes ?? []);
  const [tags, setTags] = useState<string[]>(editing?.tags ?? []);
  const [note, setNote] = useState(editing?.note ?? '');
  const [photos, setPhotos] = useState<string[]>([]);
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [removed, setRemoved] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);

  const topics = useMemo(() => topicsOfGrade(cls.grade), [cls.grade]);
  const topicStd = topicStandards(topicId);
  const chipCodes = [...topicStd.map((x) => x.standard.code), ...codes.filter((c) => !topicStd.some((x) => x.standard.code === c))];

  // 닫힐 때 남은 녹음 미리듣기 URL 정리
  const recordingsRef = useRef<Recording[]>([]);
  recordingsRef.current = recordings;
  useEffect(() => () => recordingsRef.current.forEach((r) => URL.revokeObjectURL(r.url)), []);

  const toggle = <T,>(list: T[], set: (v: T[]) => void, v: T) => set(list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);

  const pickPhotos = async () => {
    try {
      const picked = await open({
        multiple: true,
        filters: [{ name: '사진', extensions: ['jpg', 'jpeg', 'png', 'webp', 'heic'] }],
      });
      if (!picked) return;
      const list = Array.isArray(picked) ? picked : [picked];
      setPhotos((cur) => [...cur, ...list.filter((p) => !cur.includes(p))]);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const save = async () => {
    setError(null);
    setWarnings([]);
    if (selected.length === 0) {
      setError('학생을 한 명 이상 선택해 주세요.');
      return;
    }
    setSaving(true);
    try {
      const input = {
        classId: cls.id,
        observedAt: fromInputValue(observedAt),
        topicBookId: topicId,
        note: note.trim(),
        studentIds: selected,
        standardCodes: codes,
        tags,
      };
      const obs = editing ? await recordsApi.observationUpdate(editing.id, input) : await recordsApi.observationCreate(input);

      const failed: string[] = [];
      for (const id of removed) {
        try {
          await recordsApi.attachmentDelete(id);
        } catch (e) {
          failed.push(`첨부 삭제 실패: ${errorText(e)}`);
        }
      }
      for (const path of photos) {
        try {
          await recordsApi.attachmentAddFile(obs.id, path);
        } catch (e) {
          failed.push(`사진 ${basename(path)}: ${errorText(e)}`);
        }
      }
      for (const [i, r] of recordings.entries()) {
        try {
          await recordsApi.attachmentAddBytes(obs.id, 'audio', r.mime, new Uint8Array(await r.blob.arrayBuffer()));
        } catch (e) {
          failed.push(`음성 메모 ${i + 1}: ${errorText(e)}`);
        }
      }

      if (failed.length > 0) {
        // 관찰은 저장됐다. 실패한 첨부만 안내하고 양식은 유지하지 않는다(중복 저장 방지).
        setWarnings(failed);
      }
      recordings.forEach((r) => URL.revokeObjectURL(r.url));
      if (!editing) {
        setSelected([]);
        setCodes([]);
        setTags([]);
        setNote('');
        setObservedAt(toInputValue(new Date()));
      }
      setPhotos([]);
      setRecordings([]);
      setRemoved([]);
      onSaved(failed.length > 0 ? '기록은 저장했지만 일부 첨부를 저장하지 못했습니다.' : '저장했습니다.', failed.length > 0);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!editing || !window.confirm('이 기록을 휴지통으로 보낼까요?')) return;
    try {
      await recordsApi.observationDelete(editing.id);
      onDeleted?.();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const existing: Attachment[] = (editing?.attachments ?? []).filter((a) => !removed.includes(a.id));

  return (
    <div className="rec-form">
      <div className="rec-row rec-wrap">
        <label className="rec-field rec-grow">
          주제책
          <select value={topicId ?? ''} onChange={(e) => onTopicChange(e.target.value || null)}>
            <option value="">(주제책 없음)</option>
            {topics.map((t) => (
              <option key={t.id} value={t.id}>
                {t.semester}학기 {t.order}. {t.title}
              </option>
            ))}
          </select>
        </label>
        <label className="rec-field">
          날짜·시각
          <input type="datetime-local" value={observedAt} onChange={(e) => setObservedAt(e.target.value)} />
        </label>
      </div>

      <div className="rec-row rec-wrap">
        <h3 className="rec-h">학생 ({selected.length}명 선택)</h3>
        <button className="diag-secondary" onClick={() => setSelected(students.filter((s) => s.active).map((s) => s.id))}>
          모두 선택
        </button>
        <button className="diag-secondary" onClick={() => setSelected([])}>
          선택 해제
        </button>
      </div>
      <div className="rec-chips rec-students" role="group" aria-label="학생 선택">
        {students.map((s) => (
          <button
            key={s.id}
            className={selected.includes(s.id) ? 'rec-chip on' : 'rec-chip'}
            aria-pressed={selected.includes(s.id)}
            onClick={() => toggle(selected, setSelected, s.id)}
          >
            <span className="rec-chip-num">{s.number}</span>
            {s.name}
          </button>
        ))}
        {students.length === 0 && <p className="diag-note">학생이 없습니다. 포트폴리오 화면의 「학급 관리」에서 명단을 붙여 넣어 주세요.</p>}
      </div>

      <h3 className="rec-h">성취기준 (선택)</h3>
      <div className="rec-chips rec-standards" role="group" aria-label="성취기준 선택">
        {chipCodes.length === 0 && <p className="diag-hint">주제책을 고르면 성취기준이 나타납니다.</p>}
        {chipCodes.map((code) => {
          const info = standardByCode(code);
          return (
            <button
              key={code}
              className={codes.includes(code) ? 'rec-chip rec-chip-wide on' : 'rec-chip rec-chip-wide'}
              aria-pressed={codes.includes(code)}
              onClick={() => toggle(codes, setCodes, code)}
            >
              {info && <span className={`subject-chip subject-${info.subject.id}`}>{info.subject.name}</span>}
              <code>{code}</code>
              <span className="rec-chip-text">{shorten(info?.standard.statement, 22)}</span>
            </button>
          );
        })}
      </div>

      <h3 className="rec-h">활동 태그</h3>
      <div className="rec-chips" role="group" aria-label="활동 태그">
        {ACTIVITY_TAGS.map((t) => (
          <button key={t} className={tags.includes(t) ? 'rec-chip on' : 'rec-chip'} aria-pressed={tags.includes(t)} onClick={() => toggle(tags, setTags, t)}>
            {t}
          </button>
        ))}
      </div>

      <h3 className="rec-h">메모</h3>
      <textarea className="rec-textarea" aria-label="메모" rows={4} value={note} placeholder="본 것, 들은 것을 짧게 적어 주세요." onChange={(e) => setNote(e.target.value)} />

      <h3 className="rec-h">첨부</h3>
      <AttachmentView attachments={existing} onRemove={(a) => setRemoved((r) => [...r, a.id])} />
      <div className="rec-row rec-wrap">
        <button className="diag-secondary" onClick={pickPhotos}>
          사진 추가
        </button>
        <AudioRecorder onDone={(r) => setRecordings((cur) => [...cur, r])} />
      </div>
      {photos.length > 0 && (
        <ul className="rec-pending" aria-label="추가할 사진">
          {photos.map((p) => (
            <li key={p}>
              <span>사진 {basename(p)}</span>
              <button className="diag-secondary" onClick={() => setPhotos((cur) => cur.filter((x) => x !== p))}>
                빼기
              </button>
            </li>
          ))}
        </ul>
      )}
      {recordings.length > 0 && (
        <ul className="rec-pending" aria-label="추가할 음성 메모">
          {recordings.map((r) => (
            <li key={r.id}>
              <audio controls src={r.url} />
              <span>{r.seconds}초</span>
              <button
                className="diag-secondary"
                onClick={() => {
                  URL.revokeObjectURL(r.url);
                  setRecordings((cur) => cur.filter((x) => x.id !== r.id));
                }}
              >
                빼기
              </button>
            </li>
          ))}
        </ul>
      )}

      {error && <p className="diag-error">{error}</p>}
      {warnings.length > 0 && (
        <div className="diag-error" role="alert">
          <strong>저장하지 못한 첨부</strong>
          <ul>
            {warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </div>
      )}

      <div className="rec-row rec-wrap">
        <button className="diag-primary rec-save" disabled={saving} onClick={save}>
          {saving ? '저장 중…' : '저장'}
        </button>
        {editing && (
          <button className="diag-secondary" onClick={remove}>
            삭제
          </button>
        )}
        {onCancel && (
          <button className="diag-secondary" onClick={onCancel}>
            닫기
          </button>
        )}
      </div>
    </div>
  );
}
