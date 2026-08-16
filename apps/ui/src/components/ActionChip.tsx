// 行動 chip（UI 規格 V.7）。
//
// 單字母是主要載體，顏色只是加速辨識——移除顏色後資訊必須完整。
// 這是色盲可用性的實作方式，也讓 log 的純文字匯出保持可讀。

import type { ActionView } from '../../../../packages/poker-types/src/index';

const LETTER: Record<string, string> = {
  fold: 'F',
  check: 'X',
  call: 'C',
  bet: 'B',
  raiseTo: 'R',
  allIn: 'AI',
};

const COLOR: Record<string, string> = {
  fold: 'var(--action-fold)',
  check: 'var(--action-check)',
  call: 'var(--action-call)',
  bet: 'var(--action-bet)',
  raiseTo: 'var(--action-raise)',
  allIn: 'var(--action-allin)',
};

export function ActionChip({ action, bigBlind }: { action: ActionView; bigBlind: number }) {
  const letter = LETTER[action.kind] ?? '?';
  const color = COLOR[action.kind] ?? 'var(--text-secondary)';
  // 有尺度時附在字母後（V.7），以 BB 為單位
  const size = action.to != null ? (action.to / bigBlind).toFixed(1) : null;

  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 3,
        padding: '2px 6px',
        borderRadius: 'var(--radius-chip)',
        border: `1px solid ${color}`,
        color,
        fontFamily: 'var(--font-mono)',
        fontSize: 11,
        lineHeight: 1.3,
      }}
      title={`座位 ${action.seat}：${action.kind}`}
    >
      <strong>{letter}</strong>
      {size && <span style={{ opacity: 0.85 }}>{size}</span>}
    </span>
  );
}
