// 面板 A — 牌桌設定（UI 規格 A.1–A.5）。
//
// 欄位語意的權威來源是核心規格 2.1；本檔只負責呈現與編輯 UX。

import { useEffect, useState } from 'react';
import type { PowerPreviewView } from '../../../../packages/poker-types/src/index';
import { previewPower, type RunRequest } from '../api';
import { Field, NumberInput, ReadOnlyValue, Segmented, Select, TextInput, Toggle } from '../components/Field';

export const DEFAULT_REQUEST: RunRequest = {
  players: 9,
  autoRefillEnabled: true,
  autoRefillTarget: 9,
  startingStackBb: 200,
  smallBlind: 1,
  bigBlind: 2,
  anteMode: 'none',
  anteAmount: 0,
  straddleMode: 'none',
  rakeBasisPoints: 0,
  rakeCapBb: 0,
  rakeNoFlopNoDrop: false,
  handLimit: 10_000,
  masterSeed: String(Math.floor(Math.random() * 1_000_000_000)),
  heroSeat: 0,
  bots: [],
  heroOverrides: [],
  heroPostflopOverrides: { nodes: [], rules: [] },
  heroOpenTiers: { mediumAboveCentiBb: 300, largeAboveCentiBb: 600 },
};

/** 手數以 1K 為單位，範圍 1K–100K（核心規格 2.1）。 */
const HAND_STEP = 1_000;
const HAND_MIN = 1_000;
const HAND_MAX = 100_000;

/**
 * 送出前的檢查。
 *
 * 引擎自己也會驗（`SessionConfig::validate`），但那要等到背景執行緒起來
 * 才會失敗，使用者看到的是「跑了一下然後報錯」。這裡先擋掉，
 * 讓「開始執行」在設定不合法時根本按不下去。
 *
 * @returns 不合法時回傳原因，合法時回傳 null
 */
export function validateRequest(request: RunRequest): string | null {
  if (request.smallBlind >= request.bigBlind) return '小盲必須小於大盲';
  if (request.startingStackBb < 1) return '起始深度至少 1 BB';
  if (request.autoRefillEnabled && request.autoRefillTarget > request.players) {
    return '補位目標不得大於座位數';
  }
  if (request.handLimit < HAND_MIN || request.handLimit > HAND_MAX) {
    return `手數需介於 ${HAND_MIN / 1000}K 與 ${HAND_MAX / 1000}K 之間`;
  }
  if (!/^\d+$/.test(request.masterSeed)) return 'seed 必須是非負整數';
  // 跨 IPC 的欄位都是整數型別，小數會在 Tauri 反序列化時直接炸掉，
  // 錯誤訊息還是看不懂的 `invalid type: floating point`。在這裡先擋
  const integerFields: ReadonlyArray<[string, number]> = [
    ['座位數', request.players],
    ['補位目標人數', request.autoRefillTarget],
    ['起始深度', request.startingStackBb],
    ['小盲', request.smallBlind],
    ['大盲', request.bigBlind],
    ['Ante 金額', request.anteAmount],
    ['抽水比例', request.rakeBasisPoints],
    ['抽水上限', request.rakeCapBb],
    ['手數', request.handLimit],
    ['座位', request.heroSeat],
  ];
  for (const [label, value] of integerFields) {
    if (!Number.isInteger(value) || value < 0) return `${label}必須是非負整數`;
  }
  if (request.rakeBasisPoints > 10_000) return '抽水比例不得超過 100%';
  // 以下兩項引擎同樣會拒絕（`OpenTierBoundsView::to_bounds`、`BotSeatConfig`），
  // 但要等按下計算才報錯；畫面上已標紅的設定不該還能按下去
  const { mediumAboveCentiBb: medium, largeAboveCentiBb: large } = request.heroOpenTiers;
  if (
    !Number.isInteger(medium) ||
    !Number.isInteger(large) ||
    medium < 200 ||
    large > 10_000 ||
    medium >= large
  ) {
    return 'OPEN 尺度區間邊界需在 2～100 BB 之內，且中型上限大於標準上限';
  }
  for (const [index, seat] of request.bots.entries()) {
    for (const [key, value] of Object.entries(seat?.params ?? {})) {
      if (!key.startsWith('openSizeCentiBb')) continue;
      if (!Number.isInteger(value) || (value !== 0 && (value < 200 || value > 10_000))) {
        const who = index === request.heroSeat ? 'Hero' : `座位 ${index}`;
        return `${who} 的 OPEN 尺寸需為 0（繼承）或 2～100 BB`;
      }
    }
  }
  return null;
}

export function TableSetup({
  layout = 'sidebar',
  request,
  onChange,
  locked,
}: {
  layout?: 'sidebar' | 'workspace';
  request: RunRequest;
  onChange: (request: RunRequest) => void;
  locked: boolean;
}) {
  const [previews, setPreviews] = useState<PowerPreviewView[]>([]);
  const workspace = layout === 'workspace';

  // 統計效力預覽由引擎提供，手數與桌型改變時更新。
  useEffect(() => {
    let cancelled = false;
    previewPower(request.handLimit, request.players)
      .then(result => { if (!cancelled) setPreviews(result); })
      .catch(() => { if (!cancelled) setPreviews([]); });
    return () => { cancelled = true; };
  }, [request.handLimit, request.players]);

  const set = <K extends keyof RunRequest>(key: K, value: RunRequest[K]) =>
    onChange({ ...request, [key]: value });
  const blindError = request.smallBlind >= request.bigBlind ? '小盲必須小於大盲' : undefined;
  const rakeActive = request.rakeBasisPoints > 0 && request.rakeCapBb > 0;

  return (
    <div className={workspace ? 'table-setup table-setup--workspace' : 'table-setup'}>
      {workspace && <div className="table-setup__summary" aria-label="目前牌桌設定摘要">
        <div><small>桌型</small><strong>{request.players} 人桌</strong></div>
        <div><small>起始籌碼</small><strong>{request.startingStackBb} BB</strong></div>
        <div><small>小盲 / 大盲</small><strong>{request.smallBlind} / {request.bigBlind}</strong></div>
        <div><small>抽水</small><strong>{rakeActive ? `${request.rakeBasisPoints / 100}% · 上限 ${request.rakeCapBb} BB` : '不抽水'}</strong></div>
        <div><small>計算手數</small><strong>{request.handLimit.toLocaleString()} 手</strong></div>
      </div>}
      {locked && <p className="table-setup__lock" role="status">計算進行中，設定暫時鎖定。完成後可調整並重新計算。</p>}

      <Group title="桌型與籌碼" description="決定開桌人數、每位玩家的起始籌碼，以及有人離桌時是否補位。" compact={workspace}>
        <Field label="開桌人數" range="6–9 人" hint="選擇本次模擬的桌型。">
          <Segmented value={request.players} options={[6, 7, 8, 9]} disabled={locked}
            onChange={v => onChange({ ...request, players: v, autoRefillTarget: Math.min(request.autoRefillTarget, v) })} />
        </Field>
        <Field label="起始籌碼" unit="BB" range="至少 1" hint={`每位玩家開桌時持有 ${request.startingStackBb * request.bigBlind} 個籌碼單位；1 BB = ${request.bigBlind}。`}>
          <NumberInput ariaLabel="起始籌碼（BB）" value={request.startingStackBb} min={1} disabled={locked} onChange={v => set('startingStackBb', v)} />
        </Field>
        <Field label="自動補位" hint="關閉後不補人；在桌人數少於 6 人時結束該桌次。">
          <Toggle checked={request.autoRefillEnabled} disabled={locked} label="Bot 離桌後補入新 Bot" onChange={v => set('autoRefillEnabled', v)} />
        </Field>
        {request.autoRefillEnabled && <Field label="補位目標人數" range={`6–${request.players} 人`} hint="補位後維持的人數，不能超過開桌人數。">
          <Segmented value={request.autoRefillTarget} options={[6, 7, 8, 9].filter(n => n <= request.players)} disabled={locked} onChange={v => set('autoRefillTarget', v)} />
        </Field>}
        <Field label="籌碼政策" hint="目前固定使用此政策，無須設定。">
          <ReadOnlyValue value="破產離桌" note="籌碼跨手累計，不會每手重置" />
        </Field>
      </Group>

      <Group title="強制下注" description="盲注與 Ante 使用籌碼單位；起始籌碼與抽水上限則以 BB 設定。" compact={workspace}>
        <Field label="小盲 / 大盲" unit="籌碼單位" hint="兩者皆為正整數，小盲必須小於大盲。" error={blindError}>
          <div className="table-setup__blinds">
            <label>小盲<NumberInput ariaLabel="小盲" value={request.smallBlind} min={1} disabled={locked} onChange={v => set('smallBlind', v)} /></label>
            <label>大盲<NumberInput ariaLabel="大盲" value={request.bigBlind} min={1} disabled={locked} onChange={v => set('bigBlind', v)} /></label>
          </div>
        </Field>
        <Field label="Ante 模式" hint={request.anteMode === 'bbAnte' || request.anteMode === 'btnAnte' ? '代付者每手支付「每人金額 × 在桌人數」。' : request.anteMode === 'perPlayer' ? '每位在桌玩家在盲注前支付 Ante。' : '不收 Ante，只支付盲注。'}>
          <Select ariaLabel="Ante 模式" value={request.anteMode} disabled={locked} options={[
            { value: 'none', label: '無 Ante' }, { value: 'perPlayer', label: '每人支付' },
            { value: 'bbAnte', label: '大盲代付（BB Ante）' }, { value: 'btnAnte', label: '莊家代付（BTN Ante）' },
          ]} onChange={v => onChange({ ...request, anteMode: v, anteAmount: v === 'none' ? 0 : request.anteAmount })} />
        </Field>
        <Field label="Straddle" hint={`單次為 ${request.bigBlind * 2}（2 BB）；雙次再加 ${request.bigBlind * 4}（4 BB）的強制下注。`}>
          <Select ariaLabel="Straddle" value={request.straddleMode} disabled={locked} options={[
            { value: 'none', label: '無 Straddle' }, { value: 'single', label: '單次（2 BB）' }, { value: 'double', label: '雙次（2 BB、4 BB）' },
          ]} onChange={v => set('straddleMode', v)} />
        </Field>
        {request.anteMode !== 'none' && <Field label="每人 Ante 金額" unit="籌碼單位" range="0 以上的整數" hint={request.anteMode === 'perPlayer' ? `每人每手支付 ${request.anteAmount}。` : `${request.players} 人在桌時，代付者每手支付 ${request.anteAmount * request.players}。`}>
          <NumberInput ariaLabel="每人 Ante 金額" value={request.anteAmount} min={0} disabled={locked} onChange={v => set('anteAmount', v)} />
        </Field>}
      </Group>

      <Group title="抽水" description="每手依底池比例抽水，最高不超過設定上限。比例或上限為 0 時不抽水。" compact={workspace}>
        <Field label="抽水比例" unit="%" range="0–100" hint="可輸入至小數點後兩位，例如 4.5%。">
          <NumberInput ariaLabel="抽水比例（%）" value={request.rakeBasisPoints / 100} min={0} max={100} step={0.5} decimals={2} disabled={locked} onChange={v => set('rakeBasisPoints', Math.round(v * 100))} />
        </Field>
        <Field label="每手抽水上限" unit="BB" range="0 以上的整數" hint="要啟用抽水，比例與上限都需大於 0。">
          <NumberInput ariaLabel="每手抽水上限（BB）" value={request.rakeCapBb} min={0} disabled={locked} onChange={v => set('rakeCapBb', v)} />
        </Field>
        <Field label="抽水時機" hint="勾選後，只對有發出翻牌（Flop）的牌局抽水。">
          <Toggle checked={request.rakeNoFlopNoDrop} disabled={locked} label="未發翻牌不抽水" onChange={v => set('rakeNoFlopNoDrop', v)} />
        </Field>
      </Group>

      <Group title="計算設定" description="設定樣本手數與亂數種子，再按上方「計算」。完成後自動進入本次牌局重播。" compact={workspace}>
        <Field label="計算手數" range="1,000–100,000" hint="每次增減 1,000 手；較多手數有助於縮小統計誤差。">
          <div className="table-setup__hand-limit">
            <input aria-label="計算手數" type="range" min={HAND_MIN} max={HAND_MAX} step={HAND_STEP} value={request.handLimit} disabled={locked} onChange={e => set('handLimit', Number(e.target.value))} />
            <output className="num">{request.handLimit.toLocaleString()} 手</output>
          </div>
        </Field>
        <Field label="亂數種子（Seed）" range="非負整數" hint="相同種子、牌桌及策略設定可重現同一批牌局；重骰可換一批牌。">
          <div className="table-setup__seed">
            <TextInput ariaLabel="亂數種子（Seed）" value={request.masterSeed} disabled={locked} onChange={v => set('masterSeed', v.replace(/\D/g, ''))} />
            <button type="button" disabled={locked} onClick={() => set('masterSeed', String(Math.floor(Math.random() * 1_000_000_000)))}>重骰</button>
          </div>
        </Field>
        <details className="table-setup__power" open>
          <summary>查看統計精度預覽<span>估計此手數的誤差範圍與建議樣本量</span></summary>
          {previews.length > 0 ? <PowerPreview previews={previews} /> : <p className="dim">統計預覽暫時無法取得；仍可依設定手數計算。</p>}
        </details>
      </Group>
    </div>
  );
}

/**
 * A.3 統計效力預覽。
 *
 * 核心規格 5.3.1 要求效力在**設定階段**就看得見，否則使用者會在跑完
 * 之後才發現結論站不住腳。
 *
 * **每一列都給出區間，不會因為寬就換成「樣本不足」。** 區間再寬也是
 * 資訊——「這個手數下只能分辨 ±30 bb/100」本身就是結論；蓋掉它使用者
 * 什麼都不知道。達不到建議精度的列改為提示要跑到多少手。
 */
function PowerPreview({
  previews,
  compact = false,
}: {
  previews: PowerPreviewView[];
  compact?: boolean;
}) {
  if (previews.length === 0) return null;
  const target = previews[0].targetHalfWidthBb100;

  return (
    <div
      style={{
        padding: compact ? '6px 8px' : '8px 10px',
        marginBottom: compact ? 8 : 12,
        borderRadius: 'var(--radius-control)',
        border: '1px solid var(--border)',
        background: 'var(--bg-base)',
      }}
    >
      <div className="dim" style={{ fontSize: compact ? 10 : 11, marginBottom: 6 }}>
        此手數的 95% 區間半寬（估計值，實際通常更寬）
      </div>
      {previews.map((preview) => (
        <div
          key={preview.level}
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'baseline',
            gap: 8,
            fontSize: 11,
            padding: compact ? '2px 0' : '3px 0',
          }}
        >
          <span style={{ color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
            {preview.level}
          </span>
          <span
            className="num"
            style={{
              color: preview.meetsTarget ? 'var(--positive)' : 'var(--text-primary)',
              whiteSpace: 'nowrap',
            }}
          >
            {preview.halfWidthBb100 === null
              ? '—'
              : `±${preview.halfWidthBb100.toFixed(1)}`}
          </span>
          <span
            className="dim"
            style={{ fontSize: 10, minWidth: 92, textAlign: 'right', whiteSpace: 'nowrap' }}
          >
            {preview.meetsTarget
              ? `已達 ±${target}`
              : `建議 ≥ ${formatHands(preview.handsForTarget)}`}
          </span>
        </div>
      ))}
      <div
        className="dim"
        style={{ fontSize: 10, marginTop: compact ? 4 : 6, lineHeight: compact ? 1.35 : 1.5 }}
      >
        建議手數是把區間收到 ±{target} bb/100 所需的量。沒達到不代表不能跑——
        區間照樣會算出來，只是結論的說服力較弱。
      </div>
    </div>
  );
}

/** 手數以 K／M 呈現，避免 12400000 這種數字塞爆欄位。 */
function formatHands(hands: number): string {
  if (hands >= 1_000_000) return `${(hands / 1_000_000).toFixed(1)}M 手`;
  if (hands >= 1_000) return `${Math.round(hands / 1_000)}K 手`;
  return `${hands} 手`;
}

function Group({
  title,
  description,
  compact = false,
  children,
}: {
  title: string;
  description: string;
  compact?: boolean;
  children: React.ReactNode;
}) {
  return (
    <section className={compact ? 'table-setup__group' : undefined}
      style={{ marginBottom: compact ? 0 : 24 }} aria-label={title}>
      <div className="table-setup__group-heading">
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
      <div className="table-setup__fields">{children}</div>
    </section>
  );
}
