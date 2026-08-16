import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// dev server（apps/devserver）是 Tauri command 的暫時替身。
// M3 換成 Tauri 時只需替換 src/api.ts 的傳輸層，呼叫形狀不變。
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5180,
    proxy: { '/api': 'http://127.0.0.1:7801' },
  },
});
