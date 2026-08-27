// V.1 App shell 的頂部列（48px）。
//
// 規格的完整欄位是「產品名 │ 模式切換 │ 執行控制 │ 手數/底池 │ 引擎狀態」，
// 目前只實作產品名與右上角的更新說明入口；中間三段仍分散在各面板裡，
// 之後收攏進來時直接填這裡的空白區。

import { useState } from 'react';
import { ReleaseNotes } from './ReleaseNotes';

/** 由 vite.config.ts 從 tauri.conf.json 注入，避免與打包出來的版本號脫節 */
declare const __APP_VERSION__: string;

export function AppHeader() {
  const [notesOpen, setNotesOpen] = useState(false);

  return (
    <>
      <header
        style={{
          height: 48,
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          padding: '0 16px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg-surface)',
        }}
      >
        <span style={{ fontSize: 13, fontWeight: 600 }}>9max 模擬平台</span>
        <span className="num dim" style={{ fontSize: 11, textAlign: 'left' }}>
          v{__APP_VERSION__}
        </span>

        {/* 中段預留給模式切換／執行控制／手數底池／引擎狀態 */}
        <span style={{ flex: 1 }} />

        <button
          type="button"
          onClick={() => setNotesOpen(true)}
          style={{
            padding: '5px 10px',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-control)',
            background: notesOpen ? 'var(--bg-hover)' : 'transparent',
            color: 'var(--text-secondary)',
            fontFamily: 'inherit',
            fontSize: 12,
            cursor: 'pointer',
          }}
        >
          更新說明
        </button>
      </header>

      {notesOpen && <ReleaseNotes onClose={() => setNotesOpen(false)} />}
    </>
  );
}
