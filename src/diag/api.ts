// src-tauri/src/diag 의 명령과 1:1로 대응한다.
import { Channel, invoke } from '@tauri-apps/api/core';

export interface VulkanDevice {
  name: string;
  deviceType: string;
  apiVersion: string;
  driverVersion: number;
  vendorId: number;
  deviceLocalBytes: number;
}

export interface SystemReport {
  appVersion: string;
  os: string;
  arch: string;
  cpuBrand: string;
  physicalCores: number | null;
  logicalCores: number;
  totalMemoryBytes: number;
  availableMemoryBytes: number;
  modelsDir: string;
  modelsDirWritable: boolean;
  diskFreeBytes: number | null;
  webviewVersion: string | null;
  vulkan: { available: boolean; devices: VulkanDevice[]; error: string | null };
  network: { target: string; ok: boolean; detail: string; elapsedMs: number };
}

export interface EngineInfo {
  llamaBench: boolean;
  llamaServer: boolean;
  whisperCli: boolean;
  versions: Record<string, { repo: string; tag: string; asset: string }> | null;
}

/** 「평어 초안 모델」 칸에서 따로 보여 주는 모델(측정용 목록에는 넣지 않는다). */
export const EVAL_MODEL_IDS: string[] = ['qwen3-4b-q4km'];

export interface ModelStatus {
  id: string;
  label: string;
  kind: 'llm' | 'stt';
  fileName: string;
  size: number;
  license: string;
  ready: boolean;
  partialBytes: number;
}

export type DownloadProgress =
  | { stage: 'downloading'; downloaded: number; total: number; bytesPerSec: number }
  | { stage: 'verifying' };

export interface LlmBench {
  modelId: string;
  modelBytes: number;
  mode: 'gpu' | 'cpu';
  promptTokensPerSec: number;
  genTokensPerSec: number;
  backends: string;
  gpuInfo: string;
  cpuInfo: string;
  threads: number;
  gpuLayers: number;
  elapsedMs: number;
}

export interface SttBench {
  modelId: string;
  audioSeconds: number;
  elapsedMs: number;
  realTimeFactor: number;
  threads: number;
  transcript: string;
}

export const diagApi = {
  system: () => invoke<SystemReport>('diag_system'),
  engines: () => invoke<EngineInfo>('diag_engines'),
  models: () => invoke<ModelStatus[]>('diag_models'),
  download: (id: string, onProgress: (p: DownloadProgress) => void) => {
    const channel = new Channel<DownloadProgress>();
    channel.onmessage = onProgress;
    return invoke<void>('diag_download_model', { id, onProgress: channel });
  },
  importModel: (id: string, source: string) => invoke<void>('diag_import_model', { id, source }),
  benchLlm: (modelId: string, mode: 'gpu' | 'cpu') => invoke<LlmBench>('diag_bench_llm', { modelId, mode }),
  benchWhisper: (modelId: string) => invoke<SttBench>('diag_bench_whisper', { modelId }),
  saveReport: (report: unknown, summary: string) => invoke<string>('diag_save_report', { report, summary }),
};
