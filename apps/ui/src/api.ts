// IPC 傳輸層。
//
// 同一份前端程式碼要能在兩種環境下跑：
//
// - **Tauri 桌面殼**（產品形態）：透過 `window.__TAURI__` 呼叫 Rust command。
// - **瀏覽器 ＋ dev server**（開發鷹架）：打 apps/devserver 的 HTTP 端點。
//
// 兩邊的 command 名稱與參數形狀刻意一致，因此切換只發生在本檔。
//
// 型別一律引用 packages/poker-types 的產生結果，前端不自行宣告 DTO
// （實做計劃第七章：型別單一來源）。

import type {
  BotSeatConfig,
  CellOverrideView,
  HandSummaryView,
  HandView,
  HoleCardVisibility,
  ParamSpecView,
  PostflopStrategyView,
  PowerPreviewView,
  RangeMatrixView,
  RunPhase,
  RunProgress,
  RunView,
  RuntimeStatusView,
  StrategyMetaView,
  StrategyNodesView,
} from '../../../packages/poker-types/src/index';

// 進度與階段同樣由 Rust 產生，前端不自行宣告。手抄一份的話，Rust 端加了
// 欄位而 TS 端沒跟上時型別檢查照樣過，畫面卻少一塊——這正是 `phase`
// 這種「用來說明現在到底在做什麼」的欄位最容易出的事
export type { RunPhase, RunProgress };

/** 面板 A 的設定。欄位語意的權威來源是核心規格 2.1。 */
export interface RunRequest {
  players: number;
  autoRefillEnabled: boolean;
  autoRefillTarget: number;
  startingStackBb: number;
  smallBlind: number;
  bigBlind: number;
  anteMode: 'none' | 'perPlayer' | 'bbAnte' | 'btnAnte';
  anteAmount: number;
  straddleMode: 'none' | 'single' | 'double';
  /** 抽水率，萬分比（5% = 500）。UI 以 % 顯示，跨 IPC 一律整數 */
  rakeBasisPoints: number;
  rakeCapBb: number;
  rakeNoFlopNoDrop: boolean;
  handLimit: number;
  /** u64 以字串傳遞，避免在 JS 失去精度 */
  masterSeed: string;
  heroSeat: number;
  /** 逐座 Bot 設定（面板 B／C）。索引即座位序 */
  bots: BotSeatConfig[];
  /** 面板 D 的自身策略逐格覆寫。只裝在使用者座位上 */
  heroOverrides: CellOverrideView[];
}

interface TauriGlobal {
  core: { invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T> };
  event: {
    listen: <T>(name: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
  };
}

function tauri(): TauriGlobal | null {
  return (globalThis as { __TAURI__?: TauriGlobal }).__TAURI__ ?? null;
}

/** 目前是否跑在 Tauri 殼內。瀏覽器模式下執行控制不可用。 */
export function isDesktop(): boolean {
  return tauri() !== null;
}

async function http<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`IPC ${path} 失敗：${response.status}`);
  return (await response.json()) as T;
}

/**
 * 回傳 rejected promise 而非同步 throw。
 *
 * 呼叫端都是 `startRun(...).catch(...)` 的形狀；同步 throw 會在 `.catch`
 * 掛上之前就炸開，錯誤不但沒被接住，UI 還會永遠卡在「啟動中」。
 */
function desktopOnly<T>(what: string): Promise<T> {
  return Promise.reject(new Error(`${what}需要桌面版；瀏覽器模式只能檢視既有資料`));
}

// ── 執行控制（面板 E）────────────────────────────────────────────────

export function startRun(request: RunRequest): Promise<void> {
  const bridge = tauri();
  if (!bridge) return desktopOnly('執行模擬');
  // created_at 由前端提供：引擎自身不讀系統時鐘，以免時間進入可重現路徑
  return bridge.core.invoke('start_run', {
    request,
    createdAt: Math.floor(Date.now() / 1000),
  });
}

export function pauseRun(paused: boolean): Promise<void> {
  const bridge = tauri();
  if (!bridge) return desktopOnly('暫停');
  return bridge.core.invoke('pause_run', { paused });
}

export function cancelRun(): Promise<void> {
  const bridge = tauri();
  if (!bridge) return desktopOnly('取消');
  return bridge.core.invoke('cancel_run');
}

/** 訂閱進度事件，回傳解除訂閱的函式。 */
export async function onRunProgress(
  handler: (progress: RunProgress) => void,
): Promise<() => void> {
  const bridge = tauri();
  if (!bridge) return () => {};
  return bridge.event.listen<RunProgress>('run-progress', (e) => handler(e.payload));
}

export async function onRunReady(handler: () => void): Promise<() => void> {
  const bridge = tauri();
  if (!bridge) return () => {};
  return bridge.event.listen<number>('run-ready', () => handler());
}

export async function onRunFailed(handler: (message: string) => void): Promise<() => void> {
  const bridge = tauri();
  if (!bridge) return () => {};
  return bridge.event.listen<string>('run-failed', (e) => handler(e.payload));
}

// ── 設定輔助（面板 A）────────────────────────────────────────────────

/** 統計效力預覽。由引擎計算，前端不重寫算式。 */
export function previewPower(handLimit: number, players: number): Promise<PowerPreviewView[]> {
  const bridge = tauri();
  if (!bridge) return Promise.resolve([]);
  return bridge.core.invoke<PowerPreviewView[]>('preview_power', { handLimit, players });
}

// ── Bot 設定（面板 B／C）─────────────────────────────────────────────

/**
 * 21 個 Bot 參數的規格。
 *
 * 鍵、單位、上下限與說明全部從引擎取得，前端不抄一份。抄一份的後果是
 * 引擎改了範圍而 UI 不知道，使用者會拉到一個引擎會拒絕的值。
 */
export function listBotParams(): Promise<ParamSpecView[]> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<ParamSpecView[]>('list_bot_params');
  return http<ParamSpecView[]>('/api/bots/params');
}

export function listBotPresets(): Promise<BotSeatConfig[]> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<BotSeatConfig[]>('list_bot_presets');
  return http<BotSeatConfig[]>('/api/bots/presets');
}

// ── 執行環境（status bar）──────────────────────────────────────────

/**
 * 引擎版本、儲存格式版本與批次節流設定。
 *
 * 全部由引擎給，前端不抄。核心規格 3.3 要求跨 engine 版本的 run 不得合併
 * 統計——狀態列上的版本號抄錯，使用者就會拿不可比的資料下判斷。
 */
export function runtimeStatus(): Promise<RuntimeStatusView> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<RuntimeStatusView>('runtime_status');
  return http<RuntimeStatusView>('/api/runtime');
}

// ── 策略（面板 D）───────────────────────────────────────────────────

/**
 * 基準內容的來源與現況。
 *
 * 版本、是否經顧問簽核、翻後 fallback 版本全部從引擎取得。前端不自行
 * 判斷「這份內容可不可信」——那是內容的屬性，不是 UI 的意見。
 */
export function strategyMeta(): Promise<StrategyMetaView> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<StrategyMetaView>('strategy_meta');
  return http<StrategyMetaView>('/api/strategy/meta');
}

/** 翻牌／轉牌／河牌的牌面分類與合法動作欄位。 */
export function postflopStrategy(): Promise<PostflopStrategyView> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<PostflopStrategyView>('postflop_strategy');
  return http<PostflopStrategyView>('/api/strategy/postflop');
}

/** 某（桌型 × 位置）下到得了的情境與籌碼分檔。清單由引擎列舉 */
export function strategyNodes(seated: number, hero: string): Promise<StrategyNodesView> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<StrategyNodesView>('strategy_nodes', { seated, hero });
  return http<StrategyNodesView>(
    `/api/strategy/nodes?seated=${seated}&hero=${encodeURIComponent(hero)}`,
  );
}

/**
 * 一個節點的 13×13 範圍矩陣。
 *
 * 頻率、範圍寬度與加注尺度一律由引擎算。UI 只負責畫格子——重算一次
 * 就多一處會與引擎漂移的地方，而漂移的症狀是「面板顯示的範圍與 Bot
 * 實際打的不同」，完全沒有徵兆。
 */
export function strategyMatrix(
  seated: number,
  hero: string,
  bucket: string,
  scenario: string,
  overrides: CellOverrideView[],
): Promise<RangeMatrixView> {
  const bridge = tauri();
  if (bridge) {
    return bridge.core.invoke<RangeMatrixView>('strategy_matrix', {
      seated,
      hero,
      bucket,
      scenario,
      overrides,
    });
  }
  // dev server 只吃查詢字串，因此本節點的覆寫壓成 `類別:主動:跟注`。
  // 這個編碼只存在於開發鷹架，Tauri 端送的是結構化陣列
  const own = overrides.filter(
    (item) =>
      item.seated === seated &&
      item.hero === hero &&
      item.bucket === bucket &&
      item.scenario === scenario,
  );
  const ov = own.map((item) => `${item.class}:${item.aggressive}:${item.call}`).join(',');
  const query =
    `seated=${seated}&hero=${encodeURIComponent(hero)}` +
    `&bucket=${encodeURIComponent(bucket)}&scenario=${encodeURIComponent(scenario)}` +
    (ov ? `&ov=${encodeURIComponent(ov)}` : '');
  return http<RangeMatrixView>(`/api/strategy/matrix?${query}`);
}

/**
 * 某個 Bot 設定在指定節點會打出的範圍。
 *
 * 面板 C 的即時預覽：四支人格滑桿在走表的節點上是靠內容層位移作用，
 * 光看數字看不出它們把哪幾手牌搬去哪裡。規則推導與 Bot 實際決策共用
 * 引擎的同一支函式，畫出來的就是它會打的東西。
 */
export function botStrategyMatrix(
  seated: number,
  hero: string,
  bucket: string,
  scenario: string,
  bot: BotSeatConfig,
): Promise<RangeMatrixView> {
  const bridge = tauri();
  if (bridge) {
    return bridge.core.invoke<RangeMatrixView>('bot_strategy_matrix', {
      seated,
      hero,
      bucket,
      scenario,
      bot,
    });
  }
  // dev server 只吃查詢字串，參數壓成 `鍵:值`。這個編碼只存在於開發鷹架
  const packed = Object.entries(bot.params)
    .map(([key, value]) => `${key}:${value}`)
    .join(',');
  return http<RangeMatrixView>(
    `/api/strategy/bot-matrix?seated=${seated}&hero=${encodeURIComponent(hero)}` +
      `&bucket=${encodeURIComponent(bucket)}&scenario=${encodeURIComponent(scenario)}` +
      `&name=${encodeURIComponent(bot.name)}` +
      (packed ? `&p=${encodeURIComponent(packed)}` : ''),
  );
}

// ── 資料查詢（面板 F／G）─────────────────────────────────────────────

export function getRun(): Promise<RunView> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<RunView>('get_run');
  return http<RunView>('/api/run');
}

export function listHands(offset: number, limit: number): Promise<HandSummaryView[]> {
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<HandSummaryView[]>('list_hands', { offset, limit });
  return http<HandSummaryView[]>(`/api/hands?offset=${offset}&limit=${limit}`);
}

export function getHand(index: number, visibility: HoleCardVisibility): Promise<HandView> {
  // 可見範圍必須明示（核心規格 2.4）。預設不顯示未攤牌底牌
  const revealAll = visibility === 'all';
  const bridge = tauri();
  if (bridge) return bridge.core.invoke<HandView>('get_hand', { index, revealAll });
  return http<HandView>(`/api/hand?index=${index}${revealAll ? '&revealAll=1' : ''}`);
}
