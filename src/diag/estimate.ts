// 측정값으로 실제 사용 시나리오의 소요 시간을 추정한다.
// 토큰 생성 속도는 대체로 모델 파일 크기(메모리 대역폭)에 반비례한다고 보고 단순 비례로 환산한다.
// 어디까지나 대략적인 값이며, 실제 3B 모델로 다시 재는 것이 정확하다.
import type { LlmBench, SttBench } from './api';

/** 평어 생성에 쓸 3B급 Q4_K_M 모델 파일 크기(약 2GB) */
export const TARGET_LLM_BYTES = 2.0e9;
/** 학생 1명 평어: 근거 기록·지시문 입력 800토큰, 출력 250토큰(한국어 3~4문장) */
export const PROMPT_TOKENS = 800;
export const OUTPUT_TOKENS = 250;
export const CLASS_SIZE = 25;
/** 관찰 카드 녹음 한 개 길이(초) */
export const CLIP_SECONDS = 30;

export type Grade = 'good' | 'ok' | 'poor';

export interface LlmEstimate {
  source: LlmBench;
  perStudentSec: number;
  classMinutes: number;
  grade: Grade;
}

export function estimateLlm(bench: LlmBench): LlmEstimate {
  const scale = bench.modelBytes / TARGET_LLM_BYTES;
  const pp = bench.promptTokensPerSec * scale;
  const tg = bench.genTokensPerSec * scale;
  const perStudentSec = PROMPT_TOKENS / pp + OUTPUT_TOKENS / tg;
  return {
    source: bench,
    perStudentSec,
    classMinutes: (perStudentSec * CLASS_SIZE) / 60,
    grade: perStudentSec <= 60 ? 'good' : perStudentSec <= 180 ? 'ok' : 'poor',
  };
}

export function bestLlm(benches: LlmBench[]): LlmEstimate | null {
  const estimates = benches.map(estimateLlm);
  if (estimates.length === 0) return null;
  return estimates.reduce((a, b) => (b.perStudentSec < a.perStudentSec ? b : a));
}

export interface SttEstimate {
  source: SttBench;
  perClipSec: number;
  grade: Grade;
}

export function estimateStt(bench: SttBench): SttEstimate {
  // 측정값에는 모델 불러오기 시간이 포함돼 있어 약간 넉넉하게 나온다.
  const perClipSec = bench.realTimeFactor * CLIP_SECONDS;
  return { source: bench, perClipSec, grade: perClipSec <= 15 ? 'good' : perClipSec <= 45 ? 'ok' : 'poor' };
}

export const GRADE_LABEL: Record<Grade, string> = {
  good: '충분',
  ok: '가능(일괄 처리 권장)',
  poor: '부족',
};

export function formatBytes(n: number | null | undefined): string {
  if (n == null) return '알 수 없음';
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)} MB`;
  return `${(n / 1e3).toFixed(0)} KB`;
}

export function formatSeconds(s: number): string {
  if (!Number.isFinite(s)) return '측정 불가';
  if (s < 60) return `${s.toFixed(0)}초`;
  const m = Math.floor(s / 60);
  const r = Math.round(s % 60);
  return r === 0 ? `${m}분` : `${m}분 ${r}초`;
}
