// 面板 G — 逐手 Log 與重播。
//
// 原本是 M0 垂直切片的整個畫面；現在收成一個面板，
// 由 App shell 決定何時顯示。

import { useEffect, useState } from 'react';
import type {
  HandView,
  HoleCardVisibility,
  RunView,
} from '../../../../packages/poker-types/src/index';
import { getHand, getRun } from '../api';
import { ActionChip } from '../components/ActionChip';
import { HandList } from '../components/HandList';
import { TableView } from '../components/TableView';

/**
 * @param reloadToken 每次遞增就重新抓取。run 跑完後 App shell 會遞增它，
 *   讓列表換成新 run 的資料——查詢 command 一律看應用程式狀態裡的
 *   current_run，前端不需要（也拿不到）run id。
 */
export function Replay({ reloadToken, bigBlind }: { reloadToken: number; bigBlind: number }) {
  const [run, setRun] = useState<RunView | null>(null);
  const [selected, setSelected] = useState(0);
  const [hand, setHand] = useState<HandView | null>(null);
  const [visibility, setVisibility] = useState<HoleCardVisibility>('revealedOnly');
  const [error, setError] = useState<string | null>(null);

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
    <div style={{ display: 'flex', height: '100%' }}>
      {/* 左：逐手列表 */}
      <aside
        style={{
          width: 240,
          borderRight: '1px solid var(--border)',
          background: 'var(--bg-surface)',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <div style={{ padding: 12, borderBottom: '1px solid var(--border)' }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>逐手 Log</div>
          <div className="dim" style={{ fontSize: 11, marginTop: 4 }}>
            {run ? `${run.handsPlayed} 手 · ${run.instanceCount} 個桌次` : '載入中…'}
          </div>
        </div>
        <HandList
          total={run ? Number(run.handsPlayed) : 0}
          selected={selected}
          onSelect={setSelected}
          bigBlind={bigBlind}
          reloadToken={reloadToken}
        />
      </aside>

      {/* 右：牌桌與行動序列 */}
      <main style={{ flex: 1, padding: 16, overflowY: 'auto' }}>
        <header
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            marginBottom: 12,
          }}
        >
          <div>
            <h1 style={{ fontSize: 15, margin: 0 }}>
              第 <span className="num">{selected}</span> 手
              {hand && (
                <span className="dim" style={{ fontSize: 12, marginLeft: 8 }}>
                  桌次 {hand.instanceIndex} · {hand.seated} 人在桌
                </span>
              )}
            </h1>
            {run && (
              <div className="dim" style={{ fontSize: 11, marginTop: 4 }}>
                seed {run.masterSeed} · {run.rngAlgorithm}
              </div>
            )}
          </div>

          {/* 核心規格 2.4：重播是否顯示未攤牌底牌採明確設定，預設不顯示 */}
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              fontSize: 12,
              color: 'var(--text-secondary)',
              cursor: 'pointer',
            }}
          >
            <input
              type="checkbox"
              checked={visibility === 'all'}
              onChange={(e) => setVisibility(e.target.checked ? 'all' : 'revealedOnly')}
            />
            顯示未攤牌底牌
          </label>
        </header>

        {hand ? (
          <>
            <TableView hand={hand} heroSeat={run?.heroSeat ?? 0} bigBlind={bigBlind} />

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
