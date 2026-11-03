import { useCallback, useEffect, useState } from 'react';
import { errorText, recordsApi, type ClassInfo, type ImportResult, type Student } from './api';
import { currentSchoolYear, parseRoster } from './util';

interface Props {
  classes: ClassInfo[];
  /** 학급·명단이 바뀐 뒤 호출 */
  onChanged: () => Promise<void> | void;
  onClose?: () => void;
  /** 명단을 먼저 보여 줄 학급 */
  initialClassId?: string;
}

/** 학급 만들기·이름 바꾸기·보관, 명단 붙여넣기, 학생 수정·전학 처리. */
export default function ClassManager({ classes, onChanged, onClose, initialClassId }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [year, setYear] = useState(currentSchoolYear());
  const [grade, setGrade] = useState(1);
  const [name, setName] = useState('');
  const [targetId, setTargetId] = useState<string | null>(initialClassId ?? classes[0]?.id ?? null);

  const fail = useCallback((e: unknown) => setError(errorText(e)), []);

  useEffect(() => {
    if (!targetId || !classes.some((c) => c.id === targetId)) setTargetId(classes[0]?.id ?? null);
  }, [classes, targetId]);

  const create = async () => {
    setError(null);
    try {
      const c = await recordsApi.classCreate(year, grade, name);
      setName('');
      setTargetId(c.id);
      await onChanged();
    } catch (e) {
      fail(e);
    }
  };

  return (
    <section className="diag-section rec-manager" aria-label="학급 관리">
      <div className="rec-row">
        <h2>학급 관리</h2>
        {onClose && (
          <button className="diag-secondary" onClick={onClose}>
            닫기
          </button>
        )}
      </div>
      {error && <p className="diag-error">{error}</p>}

      <h3>학급 만들기</h3>
      <div className="rec-row rec-wrap">
        <label className="rec-field">
          학년도
          <input type="number" value={year} onChange={(e) => setYear(Number(e.target.value))} />
        </label>
        <label className="rec-field">
          학년
          <select value={grade} onChange={(e) => setGrade(Number(e.target.value))}>
            <option value={1}>1학년</option>
            <option value={2}>2학년</option>
          </select>
        </label>
        <label className="rec-field rec-grow">
          학급 이름
          <input value={name} placeholder="예: 2반" onChange={(e) => setName(e.target.value)} />
        </label>
        <button className="diag-primary" disabled={!name.trim()} onClick={create}>
          학급 만들기
        </button>
      </div>

      {classes.length > 0 && (
        <>
          <h3>학급 목록</h3>
          {classes.map((c) => (
            <ClassRowEditor key={c.id} info={c} onChanged={onChanged} onError={fail} />
          ))}

          <h3>명단</h3>
          <label className="rec-field">
            명단을 관리할 학급
            <select value={targetId ?? ''} onChange={(e) => setTargetId(e.target.value)}>
              {classes.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.schoolYear}학년도 {c.grade}학년 {c.name}
                </option>
              ))}
            </select>
          </label>
          {targetId && <Roster key={targetId} classId={targetId} onChanged={onChanged} onError={fail} />}
        </>
      )}
    </section>
  );
}

function ClassRowEditor({
  info,
  onChanged,
  onError,
}: {
  info: ClassInfo;
  onChanged: () => Promise<void> | void;
  onError: (e: unknown) => void;
}) {
  const [year, setYear] = useState(info.schoolYear);
  const [grade, setGrade] = useState(info.grade);
  const [name, setName] = useState(info.name);
  const dirty = year !== info.schoolYear || grade !== info.grade || name !== info.name;

  return (
    <div className="rec-row rec-wrap rec-class-row">
      <input type="number" aria-label="학년도" value={year} onChange={(e) => setYear(Number(e.target.value))} />
      <select aria-label="학년" value={grade} onChange={(e) => setGrade(Number(e.target.value))}>
        <option value={1}>1학년</option>
        <option value={2}>2학년</option>
      </select>
      <input className="rec-grow" aria-label="학급 이름" value={name} onChange={(e) => setName(e.target.value)} />
      <button
        className="diag-secondary"
        disabled={!dirty}
        onClick={() =>
          recordsApi
            .classUpdate(info.id, year, grade, name)
            .then(() => onChanged())
            .catch(onError)
        }
      >
        이름 저장
      </button>
      <button
        className="diag-secondary"
        onClick={() => {
          if (window.confirm(`'${info.name}' 학급을 보관할까요? 목록에서 사라지지만 기록은 남습니다.`)) {
            recordsApi
              .classArchive(info.id)
              .then(() => onChanged())
              .catch(onError);
          }
        }}
      >
        보관
      </button>
    </div>
  );
}

function Roster({
  classId,
  onChanged,
  onError,
}: {
  classId: string;
  onChanged: () => Promise<void> | void;
  onError: (e: unknown) => void;
}) {
  const [students, setStudents] = useState<Student[]>([]);
  const [text, setText] = useState('');
  const [result, setResult] = useState<ImportResult | null>(null);

  const reload = useCallback(() => {
    recordsApi.studentList(classId, true).then(setStudents).catch(onError);
  }, [classId, onError]);
  useEffect(reload, [reload]);

  const preview = parseRoster(text);

  const apply = async () => {
    try {
      const r = await recordsApi.studentImport(classId, text);
      setResult(r);
      setText('');
      reload();
      await onChanged();
    } catch (e) {
      onError(e);
    }
  };

  return (
    <div>
      <p className="diag-hint">
        한 줄에 한 명씩 붙여 넣으세요. 「1 김하늘」, 「1. 김하늘」, 탭으로 구분한 「번호 이름」, 이름만 적어도 됩니다. 같은 번호는 이름이 바뀝니다.
      </p>
      <textarea
        className="rec-textarea"
        aria-label="명단 붙여넣기"
        rows={6}
        value={text}
        placeholder={'1 김하늘\n2 이바다\n3 박구름'}
        onChange={(e) => setText(e.target.value)}
      />
      {text.trim() && (
        <div className="rec-preview" aria-label="명단 미리보기">
          <strong>미리보기</strong>
          <span>
            {preview.lines.map((l, i) => (
              <span key={i} className="rec-chip-static">
                {l.number ?? '자동'}번 {l.name}
              </span>
            ))}
          </span>
          {preview.skipped.length > 0 && <p className="diag-error">읽을 수 없는 줄: {preview.skipped.join(' / ')}</p>}
        </div>
      )}
      <button className="diag-primary" disabled={preview.lines.length === 0} onClick={apply}>
        명단 반영
      </button>
      {result && (
        <p className="diag-note">
          추가 {result.added}명, 이름 변경 {result.updated}명
          {result.skipped.length > 0 && `, 건너뜀 ${result.skipped.length}줄(${result.skipped.join(' / ')})`}
        </p>
      )}

      <h3>학생 {students.length}명</h3>
      {students.map((s) => (
        <StudentRow key={`${s.id}-${s.number}-${s.name}-${s.active}`} student={s} onSaved={() => { reload(); void onChanged(); }} onError={onError} />
      ))}
    </div>
  );
}

function StudentRow({ student, onSaved, onError }: { student: Student; onSaved: () => void; onError: (e: unknown) => void }) {
  const [number, setNumber] = useState(student.number);
  const [name, setName] = useState(student.name);
  const save = (active: boolean) =>
    recordsApi
      .studentUpdate(student.id, number, name, active)
      .then(onSaved)
      .catch(onError);
  return (
    <div className={student.active ? 'rec-row rec-wrap rec-student-row' : 'rec-row rec-wrap rec-student-row rec-inactive'}>
      <input type="number" aria-label="번호" value={number} onChange={(e) => setNumber(Number(e.target.value))} />
      <input className="rec-grow" aria-label="이름" value={name} onChange={(e) => setName(e.target.value)} />
      <button className="diag-secondary" disabled={number === student.number && name === student.name} onClick={() => save(student.active)}>
        저장
      </button>
      <button className="diag-secondary" onClick={() => save(!student.active)}>
        {student.active ? '전학 처리' : '다시 재학'}
      </button>
    </div>
  );
}
