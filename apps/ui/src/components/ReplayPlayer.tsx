// 重播播放器（UI 規格 G.4）。
//
// 速度 1x–10x、可暫停、可逐步。播放器只推進「目前是第幾幀」，
// 每一幀的內容都是引擎算好的完整狀態，因此拖曳與倒帶不需要重算。

import { useEffect, useRef, useState } from 'react';
import type { FrameView } from '../../../../packages/poker-types/src/index';

import { advanceTime, frameDuration } from './replayClock';

export function useReplayPlayer(frames: FrameView[], options: {
  continuous: boolean; hasNext: boolean; onAdvance: () => void;
}) {
  const [index, updateIndex] = useState(0);
  const [playing, setPlaying] = useState(true);
  const [speed, setSpeed] = useState(1);
  const [elapsed, setElapsed] = useState(0);
  const time = useRef(0);
  const finished = useRef(false);
  const latest = useRef({ index, playing, speed, options });
  latest.current = { index, playing, speed, options };
  const setIndex = (next: number) => {
    time.current = next === 0 ? 0 : frameDuration(frames[next]?.kind ?? '');
    setElapsed(time.current);
    updateIndex(next);
    finished.current = false;
  };
  useEffect(() => {
    updateIndex(0); time.current = 0; setElapsed(0); finished.current = false;
  }, [frames]);
  useEffect(() => {
    let id: number;
    let previous = performance.now();
    const tick = (now: number) => {
      const state = latest.current;
      if (state.playing && frames.length && !finished.current) {
        time.current = advanceTime(time.current, now - previous, state.speed, true);
        if (time.current >= frameDuration(frames[state.index]?.kind ?? '')) {
          if (state.index < frames.length - 1) {
            time.current = 0; updateIndex(state.index + 1);
          } else {
            finished.current = true;
            if (state.options.continuous && state.options.hasNext) state.options.onAdvance();
            else setPlaying(false);
          }
        }
        setElapsed(time.current);
      }
      previous = now;
      id = requestAnimationFrame(tick);
    };
    id = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(id);
  }, [frames]);
  return { index, setIndex, playing, setPlaying, speed, setSpeed, elapsed };
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
        aria-label="重播事件位置"
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
          min={0.5}
          step={0.5}
          max={4}
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
    case 'dealHole':
      return '依座位順序發底牌';
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
    case 'collect':
      return '收取本街下注籌碼';
    case 'refund':
      return '退回未被跟注的籌碼';
    case 'settle':
      return '結算與派彩（依紀錄合併顯示）';
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
