// 應用程式外殼。
//
// UI 規格 V.1 的版面：
//
//   [ ────────── Header 48px ────────── ]
//   [ 圖示欄 56px ][ 參數欄 300px ][ 主內容 ]
//   [ ────────── Status bar 24px ────────── ]
//
// 參數欄只放**輸入**，主內容只放**輸出**。這個分工讓「調參數 → 看結果」
// 不必來回切換畫面，也讓執行期間的鎖定範圍剛好等於參數欄。

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  cancelRun,
  isDesktop,
  onRunFailed,
  onRunProgress,
  onRunReady,
  pauseRun,
  startRun,
  type RunProgress,
  type RunRequest,
} from './api';
import type { CellOverrideView } from '../../../packages/poker-types/src/index';
import { AppHeader, type ReplayHeadline, type RunMode } from './components/AppHeader';
import { useMinimumVisible } from './motion';
import { IconRail, type RailItem } from './components/IconRail';
import { StatusBar } from './components/StatusBar';
import { BotParams } from './panels/BotParams';
import { BotSeats } from './panels/BotSeats';
import { Replay } from './panels/Replay';
import { RunControl } from './panels/RunControl';
import { Strategy } from './panels/Strategy';
import {
  DEFAULT_SELECTION,
  StrategyNav,
  type StrategySelection,
} from './panels/StrategyNav';
import { DEFAULT_REQUEST, TableSetup, validateRequest } from './panels/TableSetup';

const RAIL: readonly RailItem[] = [
  { key: 'run', glyph: '▶', label: '執行', enabled: true },
  { key: 'replay', glyph: '⏱', label: '重播', enabled: true },
  { key: 'bots', glyph: '◍', label: 'Bot', enabled: true },
  { key: 'strategy', glyph: '▦', label: '策略', enabled: true },
  { key: 'report', glyph: '◫', label: '報表', enabled: false },
];

/** 執行狀態的最短可見時間。短到看不見的回饋等於沒有回饋 */
const RUN_MIN_VISIBLE_MS = 700;

/** 中欄的標題。面板換了但標題沒換，使用者會以為自己還在上一頁 */
const PANEL_TITLE: Record<string, string> = {
  run: '牌桌設定',
  bots: 'Bot 設定',
  strategy: '策略節點',
  replay: '牌桌設定',
};

export function App() {
  const [panel, setPanel] = useState('run');
  const [request, setRequest] = useState<RunRequest>(DEFAULT_REQUEST);
  /** 面板 B 選定的座位。左欄選、右欄改，因此狀態要在兩者之上 */
  const [botSeat, setBotSeat] = useState(0);
  /** 面板 D 檢視中的翻前節點 */
  const [node, setNode] = useState<StrategySelection>(DEFAULT_SELECTION);
  const [progress, setProgress] = useState<RunProgress | null>(null);
  const [running, setRunning] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  /** E.1 的兩種模式。互動對打尚未實做，因此只會停在 batch */
  const [mode, setMode] = useState<RunMode>('batch');
  /** 頂部列的手數／底池。重播掛載時回報，換面板時清掉 */
  const [replayHeadline, setReplayHeadline] = useState<ReplayHeadline | null>(null);
  const desktop = isDesktop();
  // 一萬手在 release 約 100 毫秒跑完，進度條閃一下就消失，看起來像沒跑。
  // 這裡延長的是狀態的**可見時間**，不是計算時間——真實耗時由引擎給，
  // 完成區照實顯示，因此沒有人會被誤導成引擎比較慢
  const busy = useMinimumVisible(running, RUN_MIN_VISIBLE_MS);
  const invalid = validateRequest(request);

  // 進度事件在背景執行緒推送，因此訂閱一次即可，不隨 state 重掛
  const runningRef = useRef(running);
  runningRef.current = running;

  useEffect(() => {
    const unsubscribers: Array<() => void> = [];
    let disposed = false;

    const track = (promise: Promise<() => void>) => {
      promise
        .then((off) => {
          // 元件在訂閱完成前就卸載時，立刻解除，避免留下孤兒監聽器
          if (disposed) off();
          else unsubscribers.push(off);
        })
        .catch((error: unknown) => {
          // 訂閱失敗不能默默吞掉。最常見的原因是 Tauri capability 沒給
          // core:event:allow-listen；吞掉的話畫面會停在 0% 不動，
          // 看起來像引擎當掉，實際上只是事件永遠收不到
          if (!disposed) setFailure(`無法接收執行事件：${String(error)}`);
        });
    };

    track(
      onRunProgress((next) => {
        setProgress(next);
        if (next.finished || next.cancelled) setRunning(false);
      }),
    );
    track(
      onRunReady(() => {
        setRunning(false);
        // run 寫完才換資料來源，否則重播會讀到半成品
        setReloadToken((token) => token + 1);
        // 不自動跳到重播：跑完先讓面板 E 揭曉 E.6 的完成摘要（總手數、
        // 桌次、總時長、bb/100），跳走的話那塊等於沒人看得到。要看逐手
        // Log 由完成區的按鈕帶過去
      }),
    );
    track(
      onRunFailed((message) => {
        setRunning(false);
        setFailure(message);
      }),
    );

    return () => {
      disposed = true;
      unsubscribers.forEach((off) => off());
    };
  }, []);

  // 桌型跟著面板 A 的座位數走。使用者把桌子改成 6 人卻在 9-max 的節點上
  // 編輯策略，改的會是這個 run 根本走不到的格子
  useEffect(() => {
    setNode((current) =>
      current.seated === request.players ? current : { ...current, seated: request.players },
    );
  }, [request.players]);

  const setOverrides = useCallback(
    (heroOverrides: CellOverrideView[]) =>
      setRequest((current) => ({ ...current, heroOverrides })),
    [],
  );

  const handleStart = useCallback(() => {
    setFailure(null);
    setProgress(null);
    setRunning(true);
    startRun(request).catch((error: unknown) => {
      setRunning(false);
      setFailure(String(error));
    });
  }, [request]);

  const handlePause = useCallback(() => {
    const next = !(progress?.paused ?? false);
    pauseRun(next).catch((error: unknown) => setFailure(String(error)));
    // 暫停期間不會再推進度，因此樂觀更新，否則按鈕文字會卡住
    setProgress((current) => (current ? { ...current, paused: next } : current));
  }, [progress]);

  const handleCancel = useCallback(() => {
    cancelRun().catch((error: unknown) => setFailure(String(error)));
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', overflow: 'hidden' }}>
      <AppHeader
        mode={mode}
        onModeChange={setMode}
        desktop={desktop}
        running={busy}
        invalid={invalid}
        failure={failure}
        progress={progress}
        replay={replayHeadline}
        onStart={handleStart}
        onPause={handlePause}
        onCancel={handleCancel}
      />

      {/* 三欄工作區。minHeight 0 讓各欄自行捲動，而不是把捲軸推到最外層 */}
      <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
        <IconRail items={RAIL} active={panel} onSelect={setPanel} />

        {/* 中：參數欄。只放**輸入**——面板 A 的設定、面板 B 的座位列表、
            面板 D 的情境導航。輸出一律在右側主內容 */}
        <aside
          style={{
            width: 300,
            flexShrink: 0,
            borderRight: '1px solid var(--border)',
            background: 'var(--bg-surface)',
            overflowY: 'auto',
            padding: 16,
          }}
        >
          <h2 style={{ fontSize: 13, margin: '0 0 16px' }}>
            {PANEL_TITLE[panel] ?? '牌桌設定'}
          </h2>
          {panel === 'bots' && (
            <BotSeats
              request={request}
              onChange={setRequest}
              locked={busy}
              selected={botSeat}
              onSelect={setBotSeat}
            />
          )}
          {panel === 'strategy' && (
            <StrategyNav
              selection={node}
              onChange={setNode}
              overrideCount={request.heroOverrides.length}
            />
          )}
          {panel !== 'bots' && panel !== 'strategy' && (
            <TableSetup request={request} onChange={setRequest} locked={busy} />
          )}
        </aside>

        {/* 右：主內容 */}
        <main style={{ flex: 1, overflow: 'auto', background: 'var(--bg-base)' }}>
          {panel === 'bots' && (
            <BotParams
              request={request}
              onChange={setRequest}
              locked={busy}
              selected={botSeat}
            />
          )}
          {panel === 'strategy' && (
            <Strategy
              selection={node}
              overrides={request.heroOverrides}
              onOverridesChange={setOverrides}
              locked={busy}
            />
          )}
          {panel === 'run' && (
            <RunControl
              request={request}
              progress={progress}
              running={busy}
              desktop={desktop}
              invalid={invalid}
              failure={failure}
              onViewReplay={() => setPanel('replay')}
            />
          )}
          {panel === 'replay' && (
          <Replay
            reloadToken={reloadToken}
            bigBlind={request.bigBlind}
            onHeadline={setReplayHeadline}
          />
        )}
        </main>
      </div>

      <StatusBar mode={mode} />
    </div>
  );
}
