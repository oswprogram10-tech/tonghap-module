import { curriculum } from '../curriculum';

/** 교과별 교수학습 모형 참고(예전 「단원 개발 도구」 탭 내용 중 일부). */
export default function TeachingModels() {
  return (
    <details className="unit-ref">
      <summary>교과별 교수학습 모형 보기</summary>
      <div className="tool-grid">
        {curriculum.subjects.map((s) => (
          <article key={s.id} className="tool-card">
            <h3>
              <span className={`subject-chip subject-${s.id}`}>{s.name}</span> {s.focus} 중심
            </h3>
            <ol>
              {s.teachingModel.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ol>
          </article>
        ))}
      </div>
    </details>
  );
}
