import type { BotSeatConfig, CellOverrideView, OpenTierBoundsView, PostflopOverridesView } from '../../../packages/poker-types/src/index';
import type { RunRequest } from './api';

export const LIBRARY_KEY = '9max.strategy-library.v1';
export const DRAFT_KEY = '9max.strategy-draft.v1';

export interface StrategyContent {
  heroOverrides: CellOverrideView[];
  heroPostflopOverrides: PostflopOverridesView;
  heroOpenTiers: OpenTierBoundsView;
  heroBot: BotSeatConfig | null;
}

/** 可攜檔案只保存策略，不帶牌桌、其他座位或執行參數。 */
export interface SavedStrategy {
  format: '9max-strategy';
  schemaVersion: 1;
  id: string;
  name: string;
  version: number;
  baselineVersion: string;
  updatedAt: string;
  warnings: string[];
  content: StrategyContent;
}

export function strategyContent(request: RunRequest): StrategyContent {
  return structuredClone({
    heroOverrides: request.heroOverrides,
    heroPostflopOverrides: request.heroPostflopOverrides,
    heroOpenTiers: request.heroOpenTiers,
    heroBot: request.bots[request.heroSeat] && Object.keys(request.bots[request.heroSeat].params).length
      ? request.bots[request.heroSeat] : null,
  });
}

export function applyStrategy(request: RunRequest, content: StrategyContent): RunRequest {
  const bots = request.bots.slice();
  // 空設定同樣覆蓋原座位，避免上一份策略的 Hero 參數殘留。
  bots[request.heroSeat] = structuredClone(content.heroBot ?? { name: 'Hero', params: {} });
  return { ...request, ...structuredClone({
    heroOverrides: content.heroOverrides,
    heroPostflopOverrides: content.heroPostflopOverrides,
    heroOpenTiers: content.heroOpenTiers,
  }), bots };
}

export function strategyNameError(name: string, entries: SavedStrategy[], ownId?: string): string | null {
  const trimmed = name.trim();
  if (!trimmed || Array.from(trimmed).length > 60) return '策略名稱需為 1～60 字';
  if (entries.some(entry => entry.id !== ownId && entry.name.toLocaleLowerCase() === trimmed.toLocaleLowerCase())) {
    return '已有同名策略，請使用其他名稱';
  }
  return null;
}

export function parseStrategy(json: string): SavedStrategy {
  const value = JSON.parse(json) as SavedStrategy;
  if (!value || value.format !== '9max-strategy' || value.schemaVersion !== 1
    || typeof value.id !== 'string' || !value.id
    || typeof value.name !== 'string' || strategyNameError(value.name, [])
    || !Number.isSafeInteger(value.version) || value.version < 1
    || typeof value.baselineVersion !== 'string' || !value.baselineVersion
    || typeof value.updatedAt !== 'string' || !Number.isFinite(Date.parse(value.updatedAt))
    || !Array.isArray(value.warnings) || !value.warnings.every(warning => typeof warning === 'string')
    || !value.content || !Array.isArray(value.content.heroOverrides)
    || !Array.isArray(value.content.heroPostflopOverrides?.nodes)
    || !Array.isArray(value.content.heroPostflopOverrides?.rules)
    || !value.content.heroOpenTiers
    || !(value.content.heroBot === null || (typeof value.content.heroBot?.name === 'string'
      && value.content.heroBot.params && typeof value.content.heroBot.params === 'object'))) {
    throw new Error('策略檔案格式不符或版本不支援');
  }
  return value;
}

export function readLibrary(storage: Pick<Storage, 'getItem'>): SavedStrategy[] {
  const json = storage.getItem(LIBRARY_KEY);
  if (!json) return [];
  const entries: unknown = JSON.parse(json);
  if (!Array.isArray(entries)) throw new Error('策略庫格式不符');
  const parsed = entries.map(entry => parseStrategy(JSON.stringify(entry)));
  for (const entry of parsed) {
    if (strategyNameError(entry.name, parsed, entry.id)
      || parsed.filter(other => other.id === entry.id).length > 1) throw new Error('策略庫含有重複名稱或識別碼');
  }
  return parsed;
}
