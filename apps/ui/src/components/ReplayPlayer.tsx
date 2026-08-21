// 重播播放器（UI 規格 G.4）。
//
// 速度 1x–10x、可暫停、可逐步。播放器只推進「目前是第幾幀」，
// 每一幀的內容都是引擎算好的完整狀態，因此拖曳與倒帶不需要重算。

import { useEffect, useRef, useState } from 'react';
import type { FrameView } from '../../../../packages/poker-types/src/index';

/** 1x 時每幀停留的毫秒數。真人牌桌一個行動約一秒 */
const BASE_MS = 900;

export function useReplayPlayer(
  frames: FrameView[],
  options: {
    /** 播完一手是否自動接下一手 */
    continuous: boolean;
    /** 後面還有沒有手可以接 */
    hasNext: boolean;
    /** 要求切換到下一手 */
    onAdvance: () => void;
  },
) {
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(true);
  const [speed, setSpeed] = useState(2);
  // 用 ref 讀最新值，計時器才不必每次改速度就重建
  const latest = useRef({ playing, speed, total: frames.length });
  latest.current = { playing, speed, total: frames.length };

  // 換手就從頭播。連續模式下這一步接住上一手的播放狀態，
  // 於是一手接一手地放下去，而不是打完一手就停在收池畫面
  useEffect(() => {
    setIndex(0);
    setPlaying(true);
  }, [frames]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const { playing: on, total } = latest.current;
      if (!on) return;
      setIndex((current) => {
        if (current >= total - 1) return current;
        return current + 1;
      });
    }, BASE_MS / speed);
    return () => window.clearInterval(timer);
  }, [speed]);

  const { continuous, hasNext, onAdvance } = options;

  // 播到最後一幀：連續模式接下一手，否則停下來
  useEffect(() => {
    if (frames.length === 0 || index < frames.length - 1) return;
    if (playing && continuous && hasNext) {
      onAdvance();
      return;
    }
    setPlaying(false);
  }, [index, frames.length, playing, continuous, hasNext, onAdvance]);

  return { index, setIndex, playing, setPlaying, speed, setSpeed };
}

export function ReplayControls({
  frames,
  index,
  setIndex,
  playing,
  setPlaying,
  speed,
  setSpeed,
  continuous,
  setContinuous,
}: {
  frames: FrameView[];
  index: number;
  setIndex: (index: number) => void;
  playing: boolean;
  setPlaying: (playing: boolean) => void;
  speed: number;
  setSpeed: (speed: number) => void;
  continuous: boolean;
  setContinuous: (continuous: boolean) => void;
}) {
  const last = Math.max(0, frames.length - 1);
  const atEnd = index >= last;

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '8px 12px',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius-panel)',
        background: 'var(--bg-surface)',
        marginTop: 12,
      }}
    >
      <Button
        onClick={() => {
          // 播到底再按等於重播，否則按了不會有反應
          if (atEnd) setIndex(0);
          setPlaying(!playing);
        }}
        primary
      >
        {playing ? '暫停' : atEnd ? '重播' : '播放'}
      </Button>
      <Button
        onClick={() => {
          setPlaying(false);
          setIndex(Math.max(0, index - 1));
        }}
        disabled={index === 0}
      >
        ◀ 上一步
      </Button>
      <Button
        onClick={() => {
          setPlaying(false);
          setIndex(Math.min(last, index + 1));
        }}
        disabled={atEnd}
      >
        下一步 ▶
      </Button>

      <input
        type="range"
        min={0}
        max={last}
        value={index}
        onChange={(e) => {
          setPlaying(false);
          setIndex(Number(e.target.value));
        }}
        style={{ flex: 1 }}
      />
      {/* 明確寫「步」。只寫 21/43 會被讀成手數——這裡的單位一直是
          「這一手之內的第幾個事件」，不是第幾手 */}
      <span className="dim" style={{ fontSize: 11, minWidth: 74, whiteSpace: 'nowrap' }}>
        第 <span className="num">{index + 1}</span>/{frames.length} 步
      </span>

      <label
        style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11, cursor: 'pointer' }}
        title="播完一手自動接下一手"
      >
        <input
          type="checkbox"
          checked={continuous}
          onChange={(e) => setContinuous(e.target.checked)}
        />
        <span className="dim">連續</span>
      </label>

      <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11 }}>
        <span className="dim">速度</span>
        <input
          type="range"
          min={1}
          max={10}
          value={speed}
          onChange={(e) => setSpeed(Number(e.target.value))}
          style={{ width: 72 }}
        />
        <span className="num" style={{ minWidth: 24 }}>
          {speed}x
        </span>
      </label>
    </div>
  );
}

/** 目前這一幀在說什麼事，用一句話寫在牌桌下方。 */
export function frameCaption(frame: FrameView, positions: Array<string | null>, bigBlind: number) {
  const who = frame.seat === null ? null : (positions[frame.seat] ?? `座位 ${frame.seat}`);
  const bb = (units: number) => (units / bigBlind).toFixed(1);

  switch (frame.kind) {
    case 'ante':
      return `${who} 付 ante ${bb(frame.to ?? 0)} BB`;
    case 'smallBlind':
      return `${who} 下小盲 ${bb(frame.to ?? 0)} BB`;
    case 'bigBlind':
      return `${who} 下大盲 ${bb(frame.to ?? 0)} BB`;
    case 'straddle':
      return `${who} straddle ${bb(frame.to ?? 0)} BB`;
    case 'deal':
      return `發牌：${frame.board.slice(-1).join('')}`;
    case 'fold':
      return `${who} 棄牌`;
    case 'check':
      return `${who} 過牌`;
    case 'call':
      return `${who} 跟注至 ${bb(frame.to ?? 0)} BB`;
    case 'raiseTo':
      return `${who} 加注至 ${bb(frame.to ?? 0)} BB`;
    case 'allIn':
      return `${who} 全下 ${bb(frame.to ?? 0)} BB`;
    case 'settle':
      return '收池';
    default:
      return frame.kind;
  }
}

function Button({
  children,
  onClick,
  disabled,
  primary,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      style={{
        padding: '5px 12px',
        borderRadius: 'var(--radius-control)',
        border: `1px solid ${primary ? 'var(--accent)' : 'var(--border)'}`,
        background: primary ? 'var(--accent)' : 'transparent',
        color: primary ? 'var(--bg-base)' : 'var(--text-secondary)',
        fontFamily: 'inherit',
        fontSize: 11,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.35 : 1,
        whiteSpace: 'nowrap',
      }}
    >
      {children}
    </button>
  );
}
