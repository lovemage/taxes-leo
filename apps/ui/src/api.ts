// IPC 傳輸層。
//
// 目前打 apps/devserver 的 HTTP 端點；M3 換成 Tauri 時只需替換這個檔案的
// 實作（改成 invoke('get_hand', {...})），呼叫形狀與型別完全不變。
//
// 型別一律引用 packages/poker-types 的產生結果，前端不自行宣告 DTO
// （實做計劃第七章：型別單一來源）。

import type {
  HandSummaryView,
  HandView,
  HoleCardVisibility,
  RunView,
} from '../../../packages/poker-types/src/index';

async function get<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(`IPC ${path} 失敗：${response.status}`);
  }
  return (await response.json()) as T;
}

export function getRun(): Promise<RunView> {
  return get<RunView>('/api/run');
}

export function listHands(offset: number, limit: number): Promise<HandSummaryView[]> {
  return get<HandSummaryView[]>(`/api/hands?offset=${offset}&limit=${limit}`);
}

export function getHand(
  index: number,
  visibility: HoleCardVisibility,
): Promise<HandView> {
  // 可見範圍必須明示（核心規格 2.4）。預設不顯示未攤牌底牌，
  // 只有使用者明確開啟重播全揭露時才帶 revealAll。
  const revealAll = visibility === 'all' ? '&revealAll=1' : '';
  return get<HandView>(`/api/hand?index=${index}${revealAll}`);
}
