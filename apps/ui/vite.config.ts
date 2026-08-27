import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// 版本號的唯一來源是 tauri.conf.json——那是實際打包進安裝檔的那個數字。
// 前端若自己抄一份，改版時一定有一邊忘記改，畫面上就會顯示錯的版本。
const appVersion = JSON.parse(
  readFileSync(new URL('../desktop/tauri.conf.json', import.meta.url), 'utf8'),
).version as string;

// dev server（apps/devserver）是 Tauri command 的暫時替身。
// M3 換成 Tauri 時只需替換 src/api.ts 的傳輸層，呼叫形狀不變。
export default defineConfig({
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },
  server: {
    port: 5180,
    proxy: { '/api': 'http://127.0.0.1:7801' },
  },
});
