// IPC 傳輸層。
//
// 同一份前端程式碼要能在兩種環境下跑：
//
// - **Tauri 桌面殼**（產品形態）：透過 `window.__TAURI__.core.invoke` 呼叫
//   Rust command。
// - **瀏覽器 ＋ dev server**（開發鷹架）：打 apps/devserver 的 HTTP 端點。
//
// 兩邊的 command 名稱與參數形狀刻意一致，因此切換只發生在本檔，
// 其餘前端程式碼不需要知道自己跑在哪一種環境裡。
//
// 型別一律引用 packages/poker-types 的產生結果，前端不自行宣告 DTO
// （實做計劃第七章：型別單一來源）。

import type {
  HandSummaryView,
  HandView,
  HoleCardVisibility,
  RunView,
} from '../../../packages/poker-types/src/index';

/** Tauri 注入的全域物件。存在與否即是環境判定依據。 */
interface TauriGlobal {
  core: { invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T> };
}

function tauri(): TauriGlobal | null {
  const candidate = (globalThis as { __TAURI__?: TauriGlobal }).__TAURI__;
  return candidate ?? null;
}

/** 目前是否跑在 Tauri 殼內。供 UI 顯示環境提示。 */
export function isDesktop(): boolean {
  return tauri() !== null;
}

async function http<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(`IPC ${path} 失敗：${response.status}`);
  }
  return (await response.json()) as T;
}

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
  // 可見範圍必須明示（核心規格 2.4）。預設不顯示未攤牌底牌，
  // 只有使用者明確開啟重播全揭露時才帶 revealAll
  const revealAll = visibility === 'all';

  const bridge = tauri();
  if (bridge) return bridge.core.invoke<HandView>('get_hand', { index, revealAll });
  return http<HandView>(`/api/hand?index=${index}${revealAll ? '&revealAll=1' : ''}`);
}
