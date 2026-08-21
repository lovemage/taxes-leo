// 面板 B — 座位與 Bot 設定 ＋ 面板 C — Bot 參數（兩層 schema）。
//
// B 是座位列表（誰坐哪、用哪組設定），C 是選定座位的 21 個參數。
// 兩者放在同一個工作區：改參數時看得到是在改哪個座位，
// 不必在兩個畫面之間來回對照。

import { useEffect, useState } from 'react';
import type {
  BotSeatConfig,
  ParamSpecView,
} from '../../../../packages/poker-types/src/index';
import { listBotParams, listBotPresets, type RunRequest } from '../api';
import { NumberInput, Toggle } from '../components/Field';

export function BotSetup({
  request,
  onChange,
  locked,
}: {
  request: RunRequest;
  onChange: (request: RunRequest) => void;
  locked: boolean;
}) {
  const [specs, setSpecs] = useState<ParamSpecView[]>([]);
  const [presets, setPresets] = useState<BotSeatConfig[]>([]);
  const [selected, setSelected] = useState(0);

  useEffect(() => {
    listBotParams().then(setSpecs).catch(() => setSpecs([]));
    listBotPresets().then(setPresets).catch(() => setPresets([]));
  }, []);

  // 座位數改變時補齊或裁掉設定，讓 bots 的長度永遠等於在座人數
  useEffect(() => {
    if (request.bots.length === request.players) return;
    const next = Array.from({ length: request.players }, (_, seat) =>
      request.bots[seat] ?? { name: `座位 ${seat}`, params: {} },
    );
    onChange({ ...request, bots: next });
  }, [request, onChange]);

  if (request.bots.length !== request.players) return null;
  const seat = Math.min(selected, request.players - 1);
  const current = request.bots[seat];

  const updateSeat = (next: BotSeatConfig) =>
    onChange({
      ...request,
      bots: request.bots.map((bot, index) => (index === seat ? next : bot)),
    });

  const setParam = (key: string, value: number | null) => {
    const params = { ...current.params };
    // null 代表「恢復預設」。刪掉鍵而不是寫回預設值，
    // 這樣 RunManifest 裡只會留下真正調過的欄位
    if (value === null) delete params[key];
    else params[key] = value;
    updateSeat({ ...current, params });
  };

  const persona = specs.filter((spec) => spec.layer === 'persona' && spec.implemented);
  const behavior = specs.filter((spec) => spec.layer === 'behavior' && spec.implemented);
  // 決策路徑還沒讀到的欄位單獨列在最後並停用。混在可調的欄位裡會讓
  // 使用者拉一個不會有事的滑桿，然後以為自己調到了東西
  const pending = specs.filter((spec) => !spec.implemented);

  return (
    <>
      {specs.length === 0 && (
        <div
          style={{
            padding: '8px 10px',
            marginBottom: 12,
            border: '1px solid var(--warning)',
            borderRadius: 'var(--radius-control)',
            color: 'var(--warning)',
            fontSize: 11,
          }}
        >
          參數規格由引擎提供，瀏覽器模式取不到。請用桌面版。
        </div>
      )}

      {/* ── 面板 B：座位列表 ── */}
      <section style={{ marginBottom: 20 }}>
        <SectionTitle>座位</SectionTitle>
        <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-control)' }}>
          {request.bots.map((bot, index) => {
            const active = index === seat;
            const changed = Object.keys(bot.params).length;
            const isHero = index === request.heroSeat;
            return (
              <button
                key={index}
                type="button"
                onClick={() => setSelected(index)}
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  width: '100%',
                  padding: '6px 8px',
                  border: 'none',
                  /* 選中態：整格直角背景色塊填滿（V.4） */
                  borderRadius: 0,
                  background: active ? 'var(--bg-hover)' : 'transparent',
                  color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                  fontWeight: active ? 600 : 400,
                  fontSize: 12,
                  cursor: 'pointer',
                  fontFamily: 'inherit',
                  textAlign: 'left',
                }}
              >
                <span className="num" style={{ minWidth: 20 }}>
                  {index}
                </span>
                <span style={{ flex: 1, marginLeft: 8 }}>
                  {bot.name || '未命名'}
                  {isHero && (
                    <span style={{ color: 'var(--accent)', marginLeft: 4, fontSize: 10 }}>
                      · 你
                    </span>
                  )}
                </span>
                <span className="dim" style={{ fontSize: 10 }}>
                  {changed > 0 ? `${changed} 項` : '預設'}
                </span>
              </button>
            );
          })}
        </div>

        {presets.length > 0 && (
          <div style={{ marginTop: 10 }}>
            <div className="dim" style={{ fontSize: 10, marginBottom: 4 }}>
              套用示範組合到座位 {seat}
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
              {presets.map((preset) => (
                <button
                  key={preset.name}
                  type="button"
                  disabled={locked}
                  onClick={() => updateSeat({ ...preset })}
                  style={chipStyle(locked)}
                >
                  {preset.name}
                </button>
              ))}
              <button
                type="button"
                disabled={locked}
                onClick={() =>
                  onChange({
                    ...request,
                    bots: request.bots.map((_, index) =>
                      presets[index % presets.length]
                        ? { ...presets[index % presets.length] }
                        : { name: `座位 ${index}`, params: {} },
                    ),
                  })
                }
                style={chipStyle(locked)}
              >
                全桌輪流套用
              </button>
            </div>
            <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
              示範組合只是既有參數的組合，**不是**校準過的人格。
              核心規格的 7 組官方人格由牌手顧問定義，尚未進來。
            </div>
          </div>
        )}
      </section>

      {/* ── 面板 C：兩層參數 ── */}
      <ParamGroup
        title={`人格層 — 座位 ${seat}`}
        specs={persona}
        params={current.params}
        onChange={setParam}
        locked={locked}
      />
      <ParamGroup
        title="行為層"
        specs={behavior}
        params={current.params}
        onChange={setParam}
        locked={locked}
      />

      {pending.length > 0 && (
        <section style={{ marginBottom: 20 }}>
          <SectionTitle>尚未生效（{pending.length}）</SectionTitle>
          <div className="dim" style={{ fontSize: 10, marginBottom: 8, lineHeight: 1.5 }}>
            這些欄位已在核心規格 4.3 宣告，但決策路徑目前不會讀到，
            調了不會改變任何結果，因此停用而不是讓你白拉。
          </div>
          {pending.map((spec) => (
            <div
              key={spec.key}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'baseline',
                padding: '3px 0',
                opacity: 0.45,
                fontSize: 11,
              }}
            >
              <span>{spec.display}</span>
              <span className="num dim" style={{ fontSize: 10 }}>
                {spec.kind === 'myriad' ? `${spec.default / 100}%` : spec.default}
              </span>
            </div>
          ))}
        </section>
      )}
    </>
  );
}

function ParamGroup({
  title,
  specs,
  params,
  onChange,
  locked,
}: {
  title: string;
  specs: ParamSpecView[];
  params: Record<string, number>;
  onChange: (key: string, value: number | null) => void;
  locked: boolean;
}) {
  if (specs.length === 0) return null;
  return (
    <section style={{ marginBottom: 20 }}>
      <SectionTitle>{title}</SectionTitle>
      {specs.map((spec) => (
        <ParamField
          key={spec.key}
          spec={spec}
          value={params[spec.key]}
          onChange={(value) => onChange(spec.key, value)}
          locked={locked}
        />
      ))}
    </section>
  );
}

/**
 * 一個參數的編輯欄位。
 *
 * UX.2 要求顯示「官方預設 → 修正 → 最終生效值」。這裡以「已改」標記
 * 與「復原」按鈕表達：未改的欄位顯示預設值本身，改過的才需要對照。
 */
function ParamField({
  spec,
  value,
  onChange,
  locked,
}: {
  spec: ParamSpecView;
  value: number | undefined;
  onChange: (value: number | null) => void;
  locked: boolean;
}) {
  const effective = value ?? spec.default;
  const changed = value !== undefined && value !== spec.default;

  // 萬分比欄位以 % 顯示，跨 IPC 仍是整數（同抽水率的處理）
  const isMyriad = spec.kind === 'myriad';
  const display = isMyriad ? effective / 100 : effective;

  return (
    <div style={{ marginBottom: 10 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 3,
        }}
      >
        <label style={{ fontSize: 12 }}>
          {spec.display}
          {changed && (
            <span style={{ color: 'var(--warning)', marginLeft: 4, fontSize: 10 }}>已改</span>
          )}
        </label>
        <span className="dim" style={{ fontSize: 10, fontFamily: 'var(--font-mono)' }}>
          {isMyriad
            ? `${spec.min / 100}–${spec.max / 100}%`
            : `${spec.min}–${spec.max}`}
        </span>
      </div>

      {spec.kind === 'flag' ? (
        <Toggle
          checked={effective !== 0}
          disabled={locked}
          label={effective !== 0 ? '啟用' : '停用'}
          onChange={(on) => onChange(on ? 1 : 0)}
        />
      ) : (
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <input
            type="range"
            min={spec.min}
            max={spec.max}
            step={isMyriad ? 100 : 1}
            value={effective}
            disabled={locked}
            onChange={(e) => onChange(Number(e.target.value))}
            style={
              // 改過的參數把滑桿轉為警示色，一眼看出動過哪幾條
              { flex: 1, '--range-accent': changed ? 'var(--warning)' : 'var(--accent)' } as React.CSSProperties
            }
          />
          <div style={{ width: 76 }}>
            <NumberInput
              value={display}
              min={isMyriad ? spec.min / 100 : spec.min}
              max={isMyriad ? spec.max / 100 : spec.max}
              decimals={isMyriad ? 2 : 0}
              disabled={locked}
              onChange={(next) => onChange(isMyriad ? Math.round(next * 100) : next)}
            />
          </div>
        </div>
      )}

      <div className="dim" style={{ fontSize: 10, marginTop: 3, lineHeight: 1.4 }}>
        {spec.description}
        {changed && (
          <button
            type="button"
            disabled={locked}
            onClick={() => onChange(null)}
            style={{
              marginLeft: 6,
              padding: 0,
              border: 'none',
              background: 'none',
              color: 'var(--accent)',
              fontSize: 10,
              cursor: locked ? 'default' : 'pointer',
              fontFamily: 'inherit',
            }}
          >
            復原為 {isMyriad ? `${spec.default / 100}%` : spec.default}
          </button>
        )}
      </div>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h3
      style={{
        fontSize: 11,
        color: 'var(--text-secondary)',
        fontWeight: 600,
        margin: '0 0 8px',
        textTransform: 'uppercase',
        letterSpacing: '0.05em',
      }}
    >
      {children}
    </h3>
  );
}

function chipStyle(locked: boolean): React.CSSProperties {
  return {
    padding: '4px 8px',
    borderRadius: 'var(--radius-chip)',
    border: '1px solid var(--border)',
    background: 'transparent',
    color: 'var(--text-secondary)',
    fontSize: 11,
    fontFamily: 'inherit',
    cursor: locked ? 'default' : 'pointer',
    opacity: locked ? 0.4 : 1,
  };
}
