// 範圍矩陣的配色。
//
// 面板 D 的矩陣與面板 C 的 Bot 預覽共用這一份。各寫一套的話，同一份範圍
// 在兩個地方會是不同的顏色，使用者對照時會以為看到的是不同的內容——
// 這與 tokens.css 讓矩陣沿用顧問校準工作台配色是同一個理由。

import type { MatrixCellView } from '../../../../packages/poker-types/src/index';

/** 萬分比的滿值 */
export const FULL = 10_000;

export function cellTone(cell: MatrixCellView): React.CSSProperties {
  if (cell.aggressive >= FULL) {
    return { background: 'var(--matrix-aggressive)', color: 'var(--text-primary)' };
  }
  if (cell.aggressive > 0) {
    const ratio = ((cell.aggressive / FULL) * 0.77).toFixed(2);
    return { background: `rgba(var(--matrix-mix-rgb), ${ratio})`, color: 'var(--bg-base)' };
  }
  if (cell.call > 0) return { background: 'var(--matrix-call)', color: 'var(--text-primary)' };
  // 過牌不是棄牌。大盲無人加注時整張表都是過牌看翻牌，畫成棄牌色
  // 會讓使用者以為那一格「什麼牌都丟」
  if (cell.check > 0) return { background: 'var(--matrix-check)', color: 'var(--text-primary)' };
  return { background: 'var(--matrix-empty)', color: 'var(--text-tertiary)' };
}

/** 主導動作。差異比對用——比較「這一格現在主要在做什麼」 */
export function dominantAction(cell: MatrixCellView): 'aggressive' | 'call' | 'check' | 'fold' {
  if (cell.aggressive >= Math.max(cell.call, cell.check, cell.fold)) return 'aggressive';
  if (cell.call >= Math.max(cell.check, cell.fold)) return 'call';
  if (cell.check >= cell.fold) return 'check';
  return 'fold';
}

export const ACTION_LABEL: Record<string, string> = {
  aggressive: '加注',
  call: '跟注',
  check: '過牌',
  fold: '棄牌',
};
