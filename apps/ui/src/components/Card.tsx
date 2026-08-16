// 緊湊牌方塊（UI 規格 V.9）。
//
// 四色牌組：♠ 灰白、♥ 紅、♦ 藍、♣ 綠。
// **花色符號必須始終繪出**——紅（♥）與綠（♣）對紅綠色盲難以分辨，
// 形狀是唯一可靠的區分依據，顏色只是加速辨識。

const SUIT_SYMBOL: Record<string, string> = {
  s: '♠',
  h: '♥',
  d: '♦',
  c: '♣',
};

const SUIT_COLOR: Record<string, string> = {
  s: 'var(--suit-spade)',
  h: 'var(--suit-heart)',
  d: 'var(--suit-diamond)',
  c: 'var(--suit-club)',
};

export function Card({ code, size = 'normal' }: { code: string; size?: 'normal' | 'large' }) {
  const rank = code.slice(0, 1);
  const suit = code.slice(1, 2);
  const large = size === 'large';

  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 1,
        padding: large ? '4px 6px' : '1px 4px',
        border: `1px solid var(--border)`,
        borderRadius: 'var(--radius-chip)',
        background: 'var(--bg-raised)',
        color: SUIT_COLOR[suit] ?? 'var(--text-primary)',
        fontFamily: 'var(--font-mono)',
        fontSize: large ? 15 : 12,
        lineHeight: 1.2,
        minWidth: large ? 30 : 22,
        justifyContent: 'center',
      }}
    >
      {rank}
      {SUIT_SYMBOL[suit] ?? suit}
    </span>
  );
}

/** 未知／未公開的牌。重播預設不顯示未攤牌底牌（核心規格 2.4）。 */
export function HiddenCard({ size = 'normal' }: { size?: 'normal' | 'large' }) {
  const large = size === 'large';
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: large ? '4px 6px' : '1px 4px',
        border: `1px dashed var(--border)`,
        borderRadius: 'var(--radius-chip)',
        background: 'transparent',
        color: 'var(--text-tertiary)',
        fontFamily: 'var(--font-mono)',
        fontSize: large ? 15 : 12,
        lineHeight: 1.2,
        minWidth: large ? 30 : 22,
      }}
    >
      ?
    </span>
  );
}
