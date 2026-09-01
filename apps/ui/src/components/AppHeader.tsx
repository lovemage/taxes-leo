// V.1 App shell 的頂部列（48px）。
//
// 規格欄位：產品名 │ 模式切換 │ 執行控制 │ 手數/底池 │ 引擎狀態。
//
// 執行控制放在這裡而不是面板 E，是為了滿足 E.5：批次執行期間使用者會
// 切到面板 F／G 看既有資料，暫停與取消必須在那時候仍然按得到。同一組
// 按鈕不在兩個地方各放一份——面板 E 只留 E.3 的進度區。

import { useState } from 'react';
import type { RunProgress } from '../api';
import { ReleaseNotes } from './ReleaseNotes';

/** 由 vite.config.ts 從 tauri.conf.json 注入，避免與打包出來的版本號脫節 */
declare const __APP_VERSION__: string;

/** E.1 的兩種模式。互動對打尚未實做 */
export type RunMode = 'interactive' | 'batch';

/** 重播目前停在哪一手、底池多少。互動對打做出來之前，這是牌桌的唯一來源 */
export interface ReplayHeadline {
  hand: number;
  total: number;
  potBb: number;
}

export function AppHeader({
  mode,
  onModeChange,
  desktop,
  running,
  invalid,
  failure,
  progress,
  replay,
  onStart,
  onPause,
  onCancel,
}: {
  mode: RunMode;
  onModeChange: (mode: RunMode) => void;
  desktop: boolean;
  running: boolean;
  invalid: string | null;
  failure: string | null;
  progress: RunProgress | null;
  replay: ReplayHeadline | null;
  onStart: () => void;
  onPause: () => void;
  onCancel: () => void;
}) {
  const [notesOpen, setNotesOpen] = useState(false);
  const status = engineStatus({ desktop, running, failure, progress });

  return (
    <>
      <header
        style={{
          height: 48,
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          padding: '0 12px 0 16px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg-surface)',
          whiteSpace: 'nowrap',
        }}
      >
        <span style={{ fontSize: 13, fontWeight: 600 }}>9max 模擬平台</span>
        <span className="num dim" style={{ fontSize: 11, textAlign: 'left' }}>
          v{__APP_VERSION__}
        </span>

        <Divider />

        {/* 模式切換（E.1）。兩種模式共用同一組牌桌與 Bot 設定 */}
        <div
          style={{
            display: 'flex',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-control)',
            overflow: 'hidden',
          }}
        >
          <Segment
            label="互動對打"
            active={mode === 'interactive'}
            // 尚未實做的模式灰掉但仍列出，與 icon rail 的「報表」一致
            disabled
            title="互動對打（尚未實做）"
            onClick={() => onModeChange('interactive')}
          />
          <Segment
            label="批次模擬"
            active={mode === 'batch'}
            onClick={() => onModeChange('batch')}
          />
        </div>

        <Divider />

        {/* 執行控制（E.4） */}
        <div style={{ display: 'flex', gap: 6 }}>
          <Control
            label="計算"
            primary
            disabled={!desktop || running || invalid !== null}
            title={invalid ?? (desktop ? undefined : '執行模擬需要桌面版')}
            onClick={onStart}
          />
          <Control
            label={progress?.paused ? '繼續' : '暫停'}
            disabled={!running}
            onClick={onPause}
          />
          <Control label="取消" tone="negative" disabled={!running} onClick={onCancel} />
        </div>

        <Divider />

        {/* 手數／底池 */}
        <Readout label="手數" value={handsText(progress, running, replay)} />
        <Readout label="底池" value={replay ? `${replay.potBb.toFixed(1)} BB` : '—'} />

        <span style={{ flex: 1, minWidth: 8 }} />

        {/* 引擎狀態 */}
        <span
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            fontSize: 11,
            color: status.color,
          }}
          title={status.title}
        >
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: status.color,
              flexShrink: 0,
            }}
          />
          {status.label}
        </span>

        <Divider />

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

function Divider() {
  return (
    <span
      aria-hidden
      style={{ width: 1, height: 20, background: 'var(--border)', flexShrink: 0 }}
    />
  );
}

/** V.1 選中態：整格直角背景色塊填滿。不用左側色條，不用圓角膠囊 */
function Segment({
  label,
  active,
  disabled,
  title,
  onClick,
}: {
  label: string;
  active: boolean;
  disabled?: boolean;
  title?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      title={title}
      onClick={onClick}
      style={{
        padding: '5px 10px',
        border: 'none',
        borderRadius: 0,
        background: active ? 'var(--bg-hover)' : 'transparent',
        color: active ? 'var(--accent)' : 'var(--text-secondary)',
        fontFamily: 'inherit',
        fontSize: 12,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.35 : 1,
      }}
    >
      {label}
    </button>
  );
}

function Control({
  label,
  primary,
  tone,
  disabled,
  title,
  onClick,
}: {
  label: string;
  primary?: boolean;
  tone?: 'negative';
  disabled?: boolean;
  title?: string;
  onClick: () => void;
}) {
  const color = tone === 'negative' ? 'var(--negative)' : 'var(--accent)';
  return (
    <button
      type="button"
      disabled={disabled}
      title={title}
      onClick={onClick}
      style={{
        padding: '5px 12px',
        borderRadius: 'var(--radius-control)',
        border: `1px solid ${primary ? color : 'var(--border)'}`,
        background: primary ? color : 'transparent',
        color: primary ? 'var(--bg-base)' : tone === 'negative' ? color : 'var(--text-primary)',
        fontFamily: 'inherit',
        fontSize: 12,
        fontWeight: primary ? 600 : 400,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
      }}
    >
      {label}
    </button>
  );
}

function Readout({ label, value }: { label: string; value: string }) {
  return (
    <span style={{ display: 'flex', alignItems: 'baseline', gap: 5 }}>
      <span className="dim" style={{ fontSize: 10 }}>
        {label}
      </span>
      <span className="num" style={{ fontSize: 12, textAlign: 'left' }}>
        {value}
      </span>
    </span>
  );
}

/**
 * 執行中看批次進度，否則看重播停在哪一手。
 *
 * 批次模擬不播動畫（E.1），因此執行期間沒有「目前這一手」可言，
 * 手數指的是已完成／目標。
 */
function handsText(
  progress: RunProgress | null,
  running: boolean,
  replay: ReplayHeadline | null,
): string {
  if (running && progress) {
    return `${progress.handsDone.toLocaleString()} / ${progress.handsTotal.toLocaleString()}`;
  }
  if (replay) return `#${replay.hand.toLocaleString()} / ${replay.total.toLocaleString()}`;
  return '—';
}

function engineStatus({
  desktop,
  running,
  failure,
  progress,
}: {
  desktop: boolean;
  running: boolean;
  failure: string | null;
  progress: RunProgress | null;
}): { label: string; color: string; title?: string } {
  if (failure) return { label: '失敗', color: 'var(--negative)', title: failure };
  // 瀏覽器模式壓過閒置：那不是「準備好可以跑」，是根本跑不了
  if (!desktop) {
    return {
      label: '瀏覽器模式',
      color: 'var(--warning)',
      title: '只能檢視 dev server 既有資料。要執行模擬請開桌面版',
    };
  }
  if (running && progress?.paused) return { label: '已暫停', color: 'var(--warning)' };
  if (running && progress?.phase === 'preparingStrategy') {
    return { label: '準備內容', color: 'var(--warning)', title: '載入 equity 排序、建立 run 紀錄' };
  }
  if (running) return { label: '執行中', color: 'var(--accent)' };
  if (progress?.cancelled) return { label: '已取消', color: 'var(--text-secondary)' };
  if (progress?.finished) return { label: '已完成', color: 'var(--accent)' };
  return { label: '閒置', color: 'var(--text-secondary)' };
}
