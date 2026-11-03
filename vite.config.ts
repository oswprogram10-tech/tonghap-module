import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri 개발 서버 설정: https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // WebView2(Chromium 기반)만 대상으로 함
    target: 'chrome110',
  },
});
