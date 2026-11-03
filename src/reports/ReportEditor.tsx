import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { errorText, type ClassInfo, type Student } from '../records/api';
import { formatDate, shorten, topicTitle, topicsOfGrade } from '../records/util';
import {
  MAX_MESSAGE,
  MAX_OBSERVATIONS,
  MAX_PHOTOS,
  MAX_QUOTES,
  SHEET_HEIGHT,
  SHEET_WIDTH,
  inputOf,
  reportApi,
  type ReportDraft,
} from './api';
import ReportSheet, { sheetData, type Selection } from './ReportSheet';
import { renderPng, reportFileName } from './renderPng';

const PREVIEW_SCALE = 0.42;

const toggle = (list: string[], id: string) => (list.includes(id) ? list.filter((x) => x !== id) : [...list, id]);

/** 한 아이의 학부모 리포트를 고르고 미리 보고 이미지로 내보내는 창. */
export default function ReportEditor({ cls, student, topicId, onClose }: { cls: ClassInfo; student: Student; topicId: string | null; onClose: () => void }) {
  const topics = useMemo(() => topicsOfGrade(cls.grade), [cls.grade]);
  const [topic, setTopic] = useState<string | null>(topicId);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [draft, setDraft] = useState<ReportDraft | null>(null);
  const [sel, setSel] = useState<Selection>({ observationIds: [], attachmentIds: [], quoteAttachmentIds: [], message: '' });
  const [busy, setBusy] = useState<'save' | 'export' | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const exportRef = useRef<HTMLDivElement>(null);

  const load = useCallback(
    async (t: string | null, f: string, e: string) => {
      try {
        const d = await reportApi.draft(student.id, t, f, e);
        setDraft(d);
        setSel({ observationIds: d.observationIds, attachmentIds: d.attachmentIds, quoteAttachmentIds: d.quoteAttachmentIds, message: d.message });
        setError(null);
      } catch (err) {
        setError(errorText(err));
      }
    },
    [student.id],
  );

  // 주제책이 바뀌면 기본 기간을 다시 정하고 초안을 불러온다.
  useEffect(() => {
    let live = true;
    reportApi
      .defaultPeriod(cls.id, topic)
      .then((p) => {
        if (!live) return;
        setFrom(p.from);
        setTo(p.to);
        return load(topic, p.from, p.to);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [cls.id, topic, load]);

  const changeDate = (f: string, e: string) => {
    setFrom(f);
    setTo(e);
    if (f && e && f <= e) void load(topic, f, e);
  };

  const data = useMemo(() => (draft ? sheetData(draft, sel) : null), [draft, sel]);

  const photoOptions = useMemo(
    () => (draft?.candidates ?? []).flatMap((c) => c.photos.map((p) => ({ ...p, solo: c.soloForStudent, obsId: c.observation.id, at: c.observation.observedAt }))),
    [draft],
  );
  const quoteOptions = useMemo(
    () => (draft?.candidates ?? []).flatMap((c) => c.audioWithTranscript.map((a) => ({ ...a, at: c.observation.observedAt, solo: c.soloForStudent }))),
    [draft],
  );

  const togglePhoto = (id: string) => setSel((s) => ({ ...s, attachmentIds: toggle(s.attachmentIds, id) }));
  const groupPhotoOn = photoOptions.some((p) => !p.solo && sel.attachmentIds.includes(p.id));

  const saveDraft = async (): Promise<ReportDraft | null> => {
    if (!draft) return null;
    const saved = await reportApi.save(inputOf(draft, sel));
    setDraft(saved);
    return saved;
  };

  const onSave = async () => {
    setBusy('save');
    setInfo(null);
    try {
      await saveDraft();
      setInfo('저장했습니다.');
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const onExport = async () => {
    if (!draft || !exportRef.current) return;
    setBusy('export');
    setInfo(null);
    try {
      const path = await save({
        defaultPath: reportFileName(student.number, student.name, topicTitle(topic)),
        filters: [{ name: 'PNG 이미지', extensions: ['png'] }],
      });
      if (!path) return;
      const saved = await saveDraft();
      const bytes = await renderPng(exportRef.current);
      await reportApi.writeFile(path, bytes);
      if (saved?.id) await reportApi.markExported(saved.id);
      setInfo(`이미지를 저장했습니다: ${path}`);
      setError(null);
      if (saved) setDraft({ ...saved, exportedAt: new Date().toISOString() });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const obsFull = sel.observationIds.length >= MAX_OBSERVATIONS;
  const photoFull = sel.attachmentIds.length >= MAX_PHOTOS;
  const quoteFull = sel.quoteAttachmentIds.length >= MAX_QUOTES;

  return (
    <div className="rec-modal rp-modal" role="dialog" aria-modal="true" aria-label="학부모 리포트">
      <div className="rec-modal-body rp-body">
        <div className="rec-row rec-wrap">
          <h2 className="rec-grow">
            학부모 리포트 · {student.number}번 {student.name}
          </h2>
          <button className="diag-secondary" onClick={onClose}>
            닫기
          </button>
        </div>
        {error && <p className="diag-error">{error}</p>}
        {info && <p className="diag-note rp-info">{info}</p>}
        <p className="diag-hint">리포트에는 이 아이의 정보만 들어갑니다. 함께 기록된 친구 이름은 「친구」로 바뀌어 보입니다.</p>

        <div className="rp-layout">
          <div className="rp-settings">
            <div className="rec-row rec-wrap">
              <label className="rec-field">
                주제책
                <select value={topic ?? ''} onChange={(e) => setTopic(e.target.value || null)}>
                  <option value="">선택 안 함</option>
                  {topics.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.semester}학기 · {t.title}
                    </option>
                  ))}
                </select>
              </label>
              <label className="rec-field">
                시작일
                <input type="date" value={from} onChange={(e) => changeDate(e.target.value, to)} />
              </label>
              <label className="rec-field">
                끝일
                <input type="date" value={to} onChange={(e) => changeDate(from, e.target.value)} />
              </label>
            </div>

            <h3>
              이런 모습을 보였어요 <span className="rp-count">({sel.observationIds.length}/{MAX_OBSERVATIONS})</span>
            </h3>
            {!draft && !error && <p className="diag-note">불러오는 중…</p>}
            {draft && draft.candidates.length === 0 && <p className="diag-hint">이 기간에 이 아이의 관찰 기록이 없습니다.</p>}
            <div className="rp-list" role="group" aria-label="관찰 고르기">
              {draft?.candidates.map((c) => {
                const o = c.observation;
                const on = sel.observationIds.includes(o.id);
                return (
                  <label key={o.id} className={on ? 'rp-pick on' : 'rp-pick'}>
                    <input
                      type="checkbox"
                      checked={on}
                      disabled={!on && obsFull}
                      onChange={() => setSel((s) => ({ ...s, observationIds: toggle(s.observationIds, o.id) }))}
                    />
                    <span className="rp-pick-text">
                      <strong>{formatDate(o.observedAt)}</strong> {shorten(o.note, 60) || <em>메모 없음</em>}
                      {o.othersCount > 0 && <span className="rec-tag"> 친구 {o.othersCount}명과 함께 기록</span>}
                    </span>
                  </label>
                );
              })}
            </div>
            {obsFull && <p className="diag-hint">관찰은 {MAX_OBSERVATIONS}개까지 고를 수 있어요. 다른 걸 고르려면 먼저 하나를 빼 주세요.</p>}

            <h3>
              사진 <span className="rp-count">({sel.attachmentIds.length}/{MAX_PHOTOS})</span>
            </h3>
            <p className="diag-hint">이 아이 한 명만 기록된 관찰의 사진만 처음부터 들어갑니다. 경고 표시가 있는 사진은 다른 아이가 찍혔을 수 있습니다.</p>
            {photoOptions.length === 0 && <p className="diag-hint">고를 수 있는 사진이 없습니다.</p>}
            <div className="rp-photo-grid">
              {photoOptions.map((p) => {
                const on = sel.attachmentIds.includes(p.id);
                return (
                  <label key={p.id} className={on ? 'rp-photo on' : 'rp-photo'}>
                    <img src={convertFileSrc(p.absPath)} alt={`${formatDate(p.at)} 사진`} />
                    <span className="rp-photo-row">
                      <input type="checkbox" checked={on} disabled={!on && photoFull} onChange={() => togglePhoto(p.id)} aria-label={`${formatDate(p.at)} 사진 넣기`} />
                      <span>{formatDate(p.at)}</span>
                      {!p.solo && (
                        <span className="rp-warn" title="함께 기록된 관찰의 사진">
                          ⚠ 함께 기록
                        </span>
                      )}
                    </span>
                  </label>
                );
              })}
            </div>
            {groupPhotoOn && (
              <p className="diag-error rp-photo-notice" role="alert">
                다른 아이가 찍혀 있지 않은지 확인하세요. 함께 기록된 관찰의 사진을 넣었습니다.
              </p>
            )}

            {quoteOptions.length > 0 && (
              <>
                <h3>
                  아이 목소리 인용 <span className="rp-count">({sel.quoteAttachmentIds.length}/{MAX_QUOTES})</span>
                </h3>
                <div className="rp-list" role="group" aria-label="자막 인용 고르기">
                  {quoteOptions.map((a) => {
                    const on = sel.quoteAttachmentIds.includes(a.id);
                    return (
                      <label key={a.id} className={on ? 'rp-pick on' : 'rp-pick'}>
                        <input
                          type="checkbox"
                          checked={on}
                          disabled={!on && quoteFull}
                          onChange={() => setSel((s) => ({ ...s, quoteAttachmentIds: toggle(s.quoteAttachmentIds, a.id) }))}
                        />
                        <span className="rp-pick-text">
                          <strong>{formatDate(a.at)}</strong> “{shorten(a.transcript, 60)}”
                        </span>
                      </label>
                    );
                  })}
                </div>
              </>
            )}

            <h3>선생님이 전하는 말</h3>
            <textarea
              className="rec-textarea"
              rows={5}
              maxLength={MAX_MESSAGE}
              value={sel.message}
              onChange={(e) => setSel((s) => ({ ...s, message: e.target.value }))}
              aria-label="선생님이 전하는 말"
            />
            <p className="diag-hint">
              {sel.message.length}/{MAX_MESSAGE}자
            </p>

            <div className="rec-row rec-wrap rp-actions">
              <button className="diag-secondary" disabled={!draft || busy !== null} onClick={onSave}>
                {busy === 'save' ? '저장하는 중…' : '저장'}
              </button>
              <button className="diag-primary" disabled={!draft || busy !== null} onClick={onExport}>
                {busy === 'export' ? '이미지를 만드는 중…' : '이미지로 내보내기'}
              </button>
              {draft?.exportedAt && <span className="diag-hint">이미 내보낸 적이 있습니다({formatDate(draft.exportedAt)}).</span>}
            </div>
          </div>

          <div className="rp-preview-col">
            <p className="diag-hint">미리보기 (실제 이미지: {SHEET_WIDTH}×{SHEET_HEIGHT}px)</p>
            <div className="rp-preview" style={{ width: SHEET_WIDTH * PREVIEW_SCALE, height: SHEET_HEIGHT * PREVIEW_SCALE }} data-testid="report-preview">
              <div style={{ transform: `scale(${PREVIEW_SCALE})`, transformOrigin: 'top left', width: SHEET_WIDTH, height: SHEET_HEIGHT }}>
                {data && <ReportSheet data={data} />}
              </div>
            </div>
          </div>
        </div>

        {/* 내보내기용: 줄이지 않은 원본 크기. 화면 밖에 둔다. */}
        <div className="rp-offscreen" aria-hidden="true">
          {data && <ReportSheet ref={exportRef} data={data} />}
        </div>
      </div>
    </div>
  );
}
