// 牌桌檢視：橫向橢圓、座位環繞（實做計劃第六章第 4 點）。
//
// 使用者座位以醒目底色標示；未公開的底牌畫成佔位方塊，
// 資料本身就沒有送到前端（遮蔽發生在 IPC 邊界）。

import type { HandView } from '../../../../packages/poker-types/src/index';
import { Card, HiddenCard } from './Card';

/** 依座位數把座位平均分佈在橢圓上，起點在正下方（使用者側）。 */
function seatPosition(index: number, total: number) {
  const angle = Math.PI / 2 + (index / total) * Math.PI * 2;
  return {
    left: `${50 + 40 * Math.cos(angle)}%`,
    top: `${50 + 36 * Math.sin(angle)}%`,
  };
}

export function TableView({
  hand,
  heroSeat,
  bigBlind,
}: {
  hand: HandView;
  heroSeat: number;
  bigBlind: number;
}) {
  return (
    <div
      style={{
        position: 'relative',
        height: 420,
        borderRadius: 'var(--radius-container)',
        border: `1px solid var(--border)`,
        background: 'var(--bg-surface)',
      }}
    >
      {/* 牌桌輪廓。刻意不用絨布綠與賭場符碼（實做計劃第六章第 3 點） */}
      <div
        style={{
          position: 'absolute',
          inset: '18% 12%',
          borderRadius: '50%',
          border: `1px solid var(--border)`,
          background: 'var(--bg-base)',
        }}
      />

      {/* 公共牌與底池 */}
      <div
        style={{
          position: 'absolute',
          left: '50%',
          top: '50%',
          transform: 'translate(-50%, -50%)',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          gap: 8,
        }}
      >
        <div style={{ display: 'flex', gap: 4 }}>
          {hand.board.length > 0 ? (
            hand.board.map((code, i) => <Card key={i} code={code} size="large" />)
          ) : (
            <span className="dim">翻前結束，未發公共牌</span>
          )}
        </div>
        {hand.rake > 0 && (
          <span className="dim" style={{ fontSize: 11 }}>
            抽水 <span className="num">{(hand.rake / bigBlind).toFixed(1)}</span> BB
          </span>
        )}
      </div>

      {/* 座位 */}
      {hand.seats.map((seat, index) => {
        const isHero = seat.seat === heroSeat;
        const pos = seatPosition(index, hand.seats.length);
        return (
          <div
            key={seat.seat}
            style={{
              position: 'absolute',
              ...pos,
              transform: 'translate(-50%, -50%)',
              width: 96,
              padding: 8,
              borderRadius: 'var(--radius-panel)',
              border: `1px solid ${isHero ? 'var(--accent)' : 'var(--border)'}`,
              background: isHero ? 'var(--bg-raised)' : 'var(--bg-surface)',
              opacity: seat.occupied ? 1 : 0.35,
              textAlign: 'center',
            }}
          >
            <div
              style={{
                fontSize: 11,
                color: isHero ? 'var(--accent)' : 'var(--text-secondary)',
                fontFamily: 'var(--font-mono)',
                marginBottom: 4,
              }}
            >
              {seat.position ?? '—'}
              {isHero && ' · 你'}
            </div>
            <div style={{ display: 'flex', gap: 3, justifyContent: 'center' }}>
              {seat.occupied ? (
                seat.holeCards ? (
                  seat.holeCards.map((code, i) => <Card key={i} code={code} />)
                ) : (
                  <>
                    <HiddenCard />
                    <HiddenCard />
                  </>
                )
              ) : (
                <span className="dim" style={{ fontSize: 11 }}>
                  空位
                </span>
              )}
            </div>
            {seat.payout > 0 && (
              <div
                className="num positive"
                style={{ fontSize: 12, marginTop: 4 }}
              >
                +{(seat.payout / bigBlind).toFixed(1)}
              </div>
            )}
          </div>
        );
      })}

      {/* dead button／dead small blind 標示（規則細則 8.4） */}
      {(hand.deadButton || hand.deadSmallBlind) && (
        <div
          style={{
            position: 'absolute',
            left: 12,
            top: 12,
            padding: '4px 8px',
            borderRadius: 'var(--radius-chip)',
            border: `1px solid var(--warning)`,
            color: 'var(--warning)',
            fontSize: 11,
          }}
        >
          {hand.deadButton && 'dead button'}
          {hand.deadButton && hand.deadSmallBlind && ' · '}
          {hand.deadSmallBlind && 'dead SB'}
        </div>
      )}
    </div>
  );
}
