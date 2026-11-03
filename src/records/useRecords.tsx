import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { errorText, recordsApi, type ClassInfo } from './api';
import { topicsOfGrade } from './util';
import ClassManager from './ClassManager';

export interface RecordsCtx {
  classes: ClassInfo[];
  cls: ClassInfo;
  selectClass: (id: string) => void;
  topicId: string | null;
  selectTopic: (id: string | null) => void;
  reloadClasses: () => Promise<void>;
}

const KEY_CLASS = 'currentClassId';
const KEY_TOPIC = 'currentTopicBookId';

/**
 * 관찰 기록·포트폴리오 공통 바깥틀: 브라우저 미리보기 안내, 학급 불러오기,
 * 학급이 없으면 학급 만들기부터 보여 준다. 현재 학급·주제책은 settings에 저장한다.
 */
export function RecordsGate({ children, what = '관찰 기록' }: { children: (ctx: RecordsCtx) => ReactNode; what?: string }) {
  if (!isTauri()) {
    return (
      <main className="diag">
        <p className="diag-note">{what}은(는) 설치된 앱에서만 동작합니다. 브라우저 미리보기에서는 사용할 수 없습니다.</p>
      </main>
    );
  }
  return <Gate>{children}</Gate>;
}

function Gate({ children }: { children: (ctx: RecordsCtx) => ReactNode }) {
  const [classes, setClasses] = useState<ClassInfo[] | null>(null);
  const [classId, setClassId] = useState<string | null>(null);
  const [topicId, setTopicId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 현재 학급에 학생이 없으면 명단 붙여넣기를 먼저 보여 준다(「나중에」로 넘길 수 있다).
  const [rosterEmpty, setRosterEmpty] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);

  const checkRoster = useCallback(async (id: string | null) => {
    if (!id) return setRosterEmpty(false);
    try {
      setRosterEmpty((await recordsApi.studentList(id, true)).length === 0);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  useEffect(() => {
    void checkRoster(classId);
  }, [classId, checkRoster]);

  const reloadClasses = useCallback(async () => {
    const list = await recordsApi.classList();
    setClasses(list);
    setClassId((cur) => (cur && list.some((c) => c.id === cur) ? cur : (list[0]?.id ?? null)));
  }, []);

  useEffect(() => {
    let live = true;
    (async () => {
      try {
        const [savedClass, savedTopic] = await Promise.all([
          recordsApi.settingsGet<string>(KEY_CLASS),
          recordsApi.settingsGet<string>(KEY_TOPIC),
        ]);
        const list = await recordsApi.classList();
        if (!live) return;
        setClasses(list);
        setClassId(list.find((c) => c.id === savedClass)?.id ?? list[0]?.id ?? null);
        setTopicId(savedTopic ?? null);
      } catch (e) {
        if (live) setError(errorText(e));
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  if (error) {
    return (
      <main className="rec">
        <p className="diag-error">{error}</p>
      </main>
    );
  }
  if (classes === null) {
    return (
      <main className="rec">
        <p className="diag-note">불러오는 중…</p>
      </main>
    );
  }
  const cls = classes.find((c) => c.id === classId);
  if (!cls) {
    return (
      <main className="rec">
        <p className="diag-note">먼저 학급을 만들고 학생 명단을 붙여 넣어 주세요.</p>
        <ClassManager classes={classes} onChanged={reloadClasses} />
      </main>
    );
  }
  if (rosterEmpty && dismissed !== cls.id) {
    return (
      <main className="rec">
        <p className="diag-note">{cls.name} 학급에 학생이 없습니다. 명단을 붙여 넣어 주세요.</p>
        <ClassManager
          classes={classes}
          initialClassId={cls.id}
          onChanged={async () => {
            await reloadClasses();
            await checkRoster(cls.id);
          }}
          onClose={() => setDismissed(cls.id)}
        />
      </main>
    );
  }

  const selectClass = (id: string) => {
    setClassId(id);
    recordsApi.settingsSet(KEY_CLASS, id).catch((e) => console.error('설정 저장 실패', e));
  };
  const selectTopic = (id: string | null) => {
    setTopicId(id);
    recordsApi.settingsSet(KEY_TOPIC, id).catch((e) => console.error('설정 저장 실패', e));
  };
  // 저장된 주제책이 이 학급 학년에 없으면 선택 안 함으로 본다.
  const validTopic = topicId && topicsOfGrade(cls.grade).some((t) => t.id === topicId) ? topicId : null;

  return <>{children({ classes, cls, selectClass, topicId: validTopic, selectTopic, reloadClasses })}</>;
}
