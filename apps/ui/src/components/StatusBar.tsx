// V.1 App shell 的底部狀態列（24px）：引擎 · SQLite · 動畫模式 · 批次 UI 更新頻率。
//
// 這一列講的是「這份資料是什麼東西產生的」。核心規格 3.3 要求跨 engine
// 版本的 run 不得合併統計，因此版本號一律由引擎給，前端不抄一份。

import { useEffect, useState } from 'react';
import { runtimeStatus } from '../api';
import type { RuntimeStatusView } from '../../../../packages/poker-types/src/index';
import type { RunMode } from './AppHeader';

export function StatusBar({ mode }: { mode: RunMode }) {
  const [status, setStatus] = useState<RuntimeStatusView | null>(null);
  const [error, setError] = useState<string | null>(null);

  // 執行環境在程式生命週期內不會變，抓一次就夠
  useEffect(() => {
    runtimeStatus()
      .then(setStatus)
      .catch((e: unknown) => setError(String(e)));
  }, []);

  return (
    <footer
      style={{
        height: 24,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '0 16px',
        borderTop: '1px solid var(--border)',
        background: 'var(--bg-surface)',
        fontSize: 11,
        color: 'var(--text-tertiary)',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
      }}
    >
      {error && <span style={{ color: 'var(--negative)' }}>執行環境讀取失敗：{error}</span>}

      {status && (
        <>
          <Item label="引擎" value={`v${status.engineVersion}`} title={status.rngAlgorithm} />
          <Dot />
          <Item
            label="SQLite"
            value={`schema ${status.schemaVersion}／log v${status.logFormatVersion}`}
            title="與資料庫內的版本不符時開不起來"
          />
          <Dot />
          {/* E.1：批次模擬不播放。互動對打做出來之後這裡才會有 off／fast／normal */}
          <Item
            label="動畫"
            value={mode === 'batch' ? '不播放' : '—'}
            title={
              mode === 'batch'
                ? '批次模擬不播放牌桌動畫（E.1）'
                : '互動對打可選 off／fast／normal'
            }
          />
          <Dot />
          <Item
            label="批次 UI 更新"
            value={`每 ${status.progressEveryHands.toLocaleString()} 手`}
            title={`逐手 log 每 ${status.writeBatchHands.toLocaleString()} 手批次寫入一次`}
          />

          {/* 非正式內容要一路講到狀態列。低樣本排序畫出來的東西與正式的
              長得一模一樣，使用者沒有任何辦法自己分辨 */}
          {!status.rankingContentGrade && (
            <>
              <Dot />
              <span style={{ color: 'var(--warning)' }} title="不得作為統計依據">
                equity 排序非正式內容（{status.rankingSource}）
              </span>
            </>
          )}
        </>
      )}
    </footer>
  );
}

function Dot() {
  return <span aria-hidden>·</span>;
}

function Item({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <span style={{ display: 'flex', alignItems: 'baseline', gap: 4 }} title={title}>
      {label}
      <span className="num" style={{ color: 'var(--text-secondary)', textAlign: 'left' }}>
        {value}
      </span>
    </span>
  );
}
