// 牌桌檢視：橫向橢圓、座位環繞（實做計劃第六章第 4 點）。
//
// 使用者座位以醒目底色標示；未公開的底牌畫成佔位方塊，
// 資料本身就沒有送到前端（遮蔽發生在 IPC 邊界）。

import { useEffect, useRef, useState } from 'react';
import type { FrameView, HandView } from '../../../../packages/poker-types/src/index';
import { Card, HiddenCard } from './Card';

/** 這些幀代表有籌碼進池，籌碼動畫只在它們上面播 */
const CHIP_KINDS = new Set(['ante', 'smallBlind', 'bigBlind', 'straddle', 'call', 'raiseTo', 'allIn']);

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
  frame,
  frameIndex,
  heroSeat,
  bigBlind,
}: {
  hand: HandView;
  /** 目前播放到的一幀。公共牌、底池與各座籌碼一律取自這裡（UI 規格 G.4） */
  frame: FrameView;
  /** 第幾幀。籌碼動畫靠它換 key 重播，內容本身仍只看 `frame` */
  frameIndex: number;
  heroSeat: number;
  bigBlind: number;
}) {
  // 底池變動時頂一下。數字換了卻毫無事件感，看起來像畫面沒反應
  const [bumping, setBumping] = useState(false);
  const lastPot = useRef(frame.pot);
  useEffect(() => {
    if (frame.pot === lastPot.current) return;
    lastPot.current = frame.pot;
    setBumping(true);
    const timer = window.setTimeout(() => setBumping(false), 320);
    return () => window.clearTimeout(timer);
  }, [frame.pot]);

  // 這一幀是誰把籌碼推進去的。發牌與收池為 null，因此不會誤放
  const chipSeat =
    CHIP_KINDS.has(frame.kind) && frame.seat !== null
      ? hand.seats.findIndex((seat) => seat.seat === frame.seat)
      : -1;

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
        <div style={{ display: 'flex', gap: 4, minHeight: 62, alignItems: 'center' }}>
          {frame.board.length > 0 ? (
            frame.board.map((code, i) => <Card key={i} code={code} size="large" />)
          ) : (
            <span className="dim">未發公共牌</span>
          )}
        </div>
        <div
          className={`num${bumping ? ' pot-bump' : ''}`}
          style={{ fontSize: 18, color: 'var(--text-primary)', textAlign: 'center' }}
        >
          {(frame.pot / bigBlind).toFixed(1)}
          <span className="dim" style={{ fontSize: 11, marginLeft: 4 }}>
            BB 底池
          </span>
        </div>
        {frame.kind === 'settle' && hand.rake > 0 && (
          <span className="dim" style={{ fontSize: 11 }}>
            抽水 <span className="num">{(hand.rake / bigBlind).toFixed(1)}</span> BB
          </span>
        )}
      </div>

      {/* 座位 */}
      {hand.seats.map((seat, index) => {
        const isHero = seat.seat === heroSeat;
        const pos = seatPosition(index, hand.seats.length);
        const acting = frame.seat === seat.seat;
        const folded = frame.folded[seat.seat] ?? false;
        const committed = frame.committed[seat.seat] ?? 0;
        const stack = frame.stacks[seat.seat] ?? 0;
        return (
          <div
            key={seat.seat}
            style={{
              position: 'absolute',
              ...pos,
              transform: 'translate(-50%, -50%)',
              width: 104,
              padding: 8,
              borderRadius: 'var(--radius-panel)',
              border: `1px solid ${
                acting ? 'var(--accent)' : isHero ? 'var(--accent)' : 'var(--border)'
              }`,
              background: acting
                ? 'var(--bg-hover)'
                : isHero
                  ? 'var(--bg-raised)'
                  : 'var(--bg-surface)',
              // 棄牌者淡出但不消失，才看得出還剩誰在牌局裡
              opacity: !seat.occupied ? 0.35 : folded ? 0.4 : 1,
              textAlign: 'center',
              transition: 'opacity 120ms linear, background 120ms linear',
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
                  seat.holeCards.map((code, i) => (
                    <span
                      key={`${hand.handIndex}-${i}`}
                      className="deal-in"
                      style={{ animationDelay: `${index * 45 + i * 90}ms` }}
                    >
                      <Card code={code} />
                    </span>
                  ))
                ) : (
                  <>
                    <span
                      key={`${hand.handIndex}-h0`}
                      className="deal-in"
                      style={{ animationDelay: `${index * 45}ms` }}
                    >
                      <HiddenCard />
                    </span>
                    <span
                      key={`${hand.handIndex}-h1`}
                      className="deal-in"
                      style={{ animationDelay: `${index * 45 + 90}ms` }}
                    >
                      <HiddenCard />
                    </span>
                  </>
                )
              ) : (
                <span className="dim" style={{ fontSize: 11 }}>
                  空位
                </span>
              )}
            </div>
            {seat.occupied && (
              <div
                className="num"
                style={{ fontSize: 11, marginTop: 4, color: 'var(--text-secondary)' }}
              >
                {(stack / bigBlind).toFixed(1)}
                <span className="dim" style={{ marginLeft: 2 }}>BB</span>
              </div>
            )}
            {committed > 0 && (
              <div
                className="num"
                style={{ fontSize: 11, color: 'var(--warning)' }}
              >
                投入 {(committed / bigBlind).toFixed(1)}
              </div>
            )}
            {frame.kind === 'settle' && seat.payout > 0 && (
              <div className="num positive" style={{ fontSize: 12, marginTop: 2 }}>
                +{(seat.payout / bigBlind).toFixed(1)}
              </div>
            )}
          </div>
        );
      })}

      {/* 籌碼滑進底池。key 帶幀序，因此每一幀重播一次。
          刻意**不標金額**：`frame.to` 是「跟到／加注到」的本街累計額，
          與底池這一幀實際增加的量不同（前面已投入的不會再進去一次）。
          把它印在飛向底池的籌碼上會被讀成進池金額，而正確的差額 UI
          不得自己算（G.4：底池與各座籌碼不得由 UI 重算）。金額看座位的
          「投入」與行動列，那兩處都是引擎給的。 */}
      {chipSeat >= 0 && frame.to !== null && (
        <span
          key={frameIndex}
          className="chip-fly"
          aria-hidden
          style={
            {
              '--from-left': seatPosition(chipSeat, hand.seats.length).left,
              '--from-top': seatPosition(chipSeat, hand.seats.length).top,
            } as React.CSSProperties
          }
        />
      )}

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
