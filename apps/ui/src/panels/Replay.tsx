// 面板 G — 逐手 Log 與重播。

import { useCallback, useEffect, useState } from 'react';
import type {
  HandView,
  HoleCardVisibility,
  RunView,
} from '../../../../packages/poker-types/src/index';
import { getHand, getRun } from '../api';
import type { ReplayHeadline } from '../components/AppHeader';
import { ActionChip } from '../components/ActionChip';
import { HandList } from '../components/HandList';
import { frameCaption, ReplayControls, useReplayPlayer } from '../components/ReplayPlayer';
import { TableView } from '../components/TableView';

/** 手牌未載入時的空幀，讓 hook 的呼叫順序不隨資料變動 */
const NO_FRAMES: never[] = [];

/**
 * @param reloadToken 每次遞增就重新抓取。run 跑完後 App shell 會遞增它，
 *   讓列表換成新 run 的資料——查詢 command 一律看應用程式狀態裡的
 *   current_run，前端不需要（也拿不到）run id。
 */
export function Replay({
  reloadToken,
  bigBlind,
  onHeadline,
}: {
  reloadToken: number;
  bigBlind: number;
  /** 回報頂部列要顯示的手數／底池（V.1）。身分必須穩定，否則每次 render 重跑 */
  onHeadline: (headline: ReplayHeadline | null) => void;
}) {
  const [run, setRun] = useState<RunView | null>(null);
  const [selected, setSelected] = useState(0);
  const [hand, setHand] = useState<HandView | null>(null);
  // 預設全部攤開（核心規格 2.4）。這是策略分析工具，複盤就是要看清楚
  // 每個位置手上是什麼；遮住等於讓工具失去意義。
  //
  // 隔離的約束對象是引擎的決策路徑，不是這個畫面——牌桌上的 Bot 拿不到
  // 他人底牌（DecisionView 結構上就沒有那個欄位），桌外複盤的人看得到
  const [visibility, setVisibility] = useState<HoleCardVisibility>('all');
  const [error, setError] = useState<string | null>(null);
  const [listOpen, setListOpen] = useState(false);
  const [continuous, setContinuous] = useState(true);

  const total = run ? Number(run.handsPlayed) : 0;

  useEffect(() => {
    setError(null);
    getRun()
      .then((runView) => {
        setRun(runView);
        setSelected(0);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [reloadToken]);

  useEffect(() => {
    if (!run) return;
    getHand(selected, visibility)
      .then(setHand)
      .catch((e: unknown) => setError(String(e)));
  }, [selected, visibility, run]);

  // 身分穩定，否則播放器的結束判定會每次 render 重跑
  const advance = useCallback(() => setSelected((current) => current + 1), []);

  const player = useReplayPlayer(hand?.frames ?? NO_FRAMES, {
    continuous,
    hasNext: selected + 1 < total,
    onAdvance: advance,
  });

  const frame = hand && hand.frames.length > 0
    ? hand.frames[Math.min(player.index, hand.frames.length - 1)]
    : null;

  // 手數編號與面板內的 header 用同一個基準，兩處對不上會讓人以為看的不是同一手
  useEffect(() => {
    onHeadline(frame && total > 0 ? { hand: selected, total, potBb: frame.pot / bigBlind } : null);
  }, [frame, selected, total, bigBlind, onHeadline]);

  // 切走面板時清掉，否則頂部列會停在上一次看到的數字
  useEffect(() => () => onHeadline(null), [onHeadline]);

  if (error) {
    return (
      <div style={{ padding: 24 }}>
        <h1 style={{ fontSize: 16 }}>沒有可重播的資料</h1>
        <p className="muted">{error}</p>
        <p className="dim">請先在「執行」面板跑一個 run。</p>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* ── Header：逐手 Log 收在這裡，點擊才展開 ── */}
      <header
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          padding: '8px 16px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg-surface)',
          flexShrink: 0,
        }}
      >
        <button
          type="button"
          onClick={() => setListOpen(!listOpen)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '5px 10px',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-control)',
            background: listOpen ? 'var(--bg-hover)' : 'transparent',
            color: 'var(--text-primary)',
            fontFamily: 'inherit',
            fontSize: 12,
            cursor: 'pointer',
          }}
        >
          <span style={{ fontSize: 10 }}>{listOpen ? '▾' : '▸'}</span>
          逐手 Log
          <span className="num dim">
            #{selected}
          </span>
          <span className="dim" style={{ fontSize: 11 }}>
            / {total.toLocaleString()} 手
          </span>
        </button>

        <div style={{ flex: 1, fontSize: 12, color: 'var(--text-secondary)' }}>
          {hand && (
            <>
              桌次 {hand.instanceIndex} · {hand.seated} 人在桌
              {run && (
                <span className="dim" style={{ marginLeft: 10, fontSize: 11 }}>
                  seed {run.masterSeed}
                </span>
              )}
            </>
          )}
        </div>

        <label
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            fontSize: 12,
            color: 'var(--text-secondary)',
            cursor: 'pointer',
            whiteSpace: 'nowrap',
          }}
          title="切到限制模式後，未亮出的底牌不會傳到前端，可用來驗證自己的讀牌"
        >
          <input
            type="checkbox"
            checked={visibility === 'revealedOnly'}
            onChange={(e) => setVisibility(e.target.checked ? 'revealedOnly' : 'all')}
          />
          只顯示實際亮出的底牌
        </label>
      </header>

      {listOpen && (
        <div
          style={{
            height: 280,
            display: 'flex',
            flexDirection: 'column',
            borderBottom: '1px solid var(--border)',
            background: 'var(--bg-surface)',
            flexShrink: 0,
          }}
        >
          <HandList
            total={total}
            selected={selected}
            onSelect={(index) => {
              setSelected(index);
              setListOpen(false);
            }}
            bigBlind={bigBlind}
            reloadToken={reloadToken}
          />
        </div>
      )}

      {/* ── 牌桌與行動序列 ── */}
      <main style={{ flex: 1, padding: 16, overflowY: 'auto' }}>
        {hand && frame ? (
          <>
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'baseline',
                marginBottom: 8,
              }}
            >
              <h1 style={{ fontSize: 15, margin: 0 }}>
                第 <span className="num">{selected}</span> 手
              </h1>
              {visibility === 'all' && (
                <span className="dim" style={{ fontSize: 11 }}>
                  全揭露模式
                </span>
              )}
            </div>

            <TableView
              hand={hand}
              frame={frame}
              heroSeat={run?.heroSeat ?? 0}
              bigBlind={bigBlind}
            />

            <div
              style={{
                marginTop: 10,
                fontSize: 13,
                color: 'var(--text-primary)',
                textAlign: 'center',
                minHeight: 18,
              }}
            >
              {frameCaption(frame, hand.seats.map((seat) => seat.position), bigBlind)}
            </div>

            <ReplayControls
              frames={hand.frames}
              {...player}
              continuous={continuous}
              setContinuous={setContinuous}
            />

            <section
              style={{
                marginTop: 16,
                padding: 12,
                borderRadius: 'var(--radius-panel)',
                border: '1px solid var(--border)',
                background: 'var(--bg-surface)',
              }}
            >
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>行動序列</div>
              {(['preflop', 'flop', 'turn', 'river'] as const).map((street) => {
                const actions = hand.actions.filter((a) => a.street === street);
                if (actions.length === 0) return null;
                return (
                  <div
                    key={street}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 6,
                      padding: '6px 0',
                      borderTop: '1px solid var(--border)',
                      flexWrap: 'wrap',
                    }}
                  >
                    <span
                      className="muted"
                      style={{ width: 64, fontSize: 11, fontFamily: 'var(--font-mono)' }}
                    >
                      {street}
                    </span>
                    {actions.map((action, i) => (
                      <ActionChip key={i} action={action} bigBlind={bigBlind} />
                    ))}
                  </div>
                );
              })}
            </section>
          </>
        ) : (
          <p className="dim">載入中…</p>
        )}
      </main>
    </div>
  );
}
