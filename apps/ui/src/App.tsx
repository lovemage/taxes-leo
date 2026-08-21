// 應用程式外殼。
//
// UI 規格 V.1 的三欄式版面：
//
//   [ 圖示欄 56px ][ 參數欄 300px ][ 主內容 ]
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
import { IconRail, type RailItem } from './components/IconRail';
import { Replay } from './panels/Replay';
import { RunControl } from './panels/RunControl';
import { DEFAULT_REQUEST, TableSetup, validateRequest } from './panels/TableSetup';

const RAIL: readonly RailItem[] = [
  { key: 'run', glyph: '▶', label: '執行', enabled: true },
  { key: 'replay', glyph: '⏱', label: '重播', enabled: true },
  { key: 'bots', glyph: '◍', label: 'Bot', enabled: false },
  { key: 'strategy', glyph: '▦', label: '策略', enabled: false },
  { key: 'report', glyph: '◫', label: '報表', enabled: false },
];

export function App() {
  const [panel, setPanel] = useState('run');
  const [request, setRequest] = useState<RunRequest>(DEFAULT_REQUEST);
  const [progress, setProgress] = useState<RunProgress | null>(null);
  const [running, setRunning] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const desktop = isDesktop();
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
        // 跑完就是要看結果，停在進度條上等使用者自己點一下沒有意義
        setPanel('replay');
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
    <div style={{ display: 'flex', height: '100vh', overflow: 'hidden' }}>
      <IconRail items={RAIL} active={panel} onSelect={setPanel} />

      {/* 中：參數欄。目前只有面板 A，其餘面板進來後在這裡分支 */}
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
        <h2 style={{ fontSize: 13, margin: '0 0 16px' }}>牌桌設定</h2>
        <TableSetup request={request} onChange={setRequest} locked={running} />
      </aside>

      {/* 右：主內容 */}
      <main style={{ flex: 1, overflow: 'auto', background: 'var(--bg-base)' }}>
        {panel === 'run' ? (
          <RunControl
            request={request}
            progress={progress}
            running={running}
            desktop={desktop}
            invalid={invalid}
            failure={failure}
            onStart={handleStart}
            onPause={handlePause}
            onCancel={handleCancel}
          />
        ) : (
          <Replay reloadToken={reloadToken} bigBlind={request.bigBlind} />
        )}
      </main>
    </div>
  );
}
