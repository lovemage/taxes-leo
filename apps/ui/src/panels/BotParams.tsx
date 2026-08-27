// 面板 C — Bot 參數（右側資訊窗）。
//
// 座位在左欄選，參數在這裡改。放在主內容區有兩個理由：參數有 21 個，
// 300px 的參數欄裝不下而必須一直捲動；而且改參數時要看得到自己在改
// 哪個座位，兩者分欄比上下堆疊更清楚。

import { useEffect, useState } from 'react';
import type { ParamSpecView } from '../../../../packages/poker-types/src/index';
import { listBotParams, type RunRequest } from '../api';
import { BotRangePreview } from '../components/BotRangePreview';
import { NumberInput, TextInput, Toggle } from '../components/Field';

export function BotParams({
  request,
  onChange,
  locked,
  selected,
}: {
  request: RunRequest;
  onChange: (request: RunRequest) => void;
  locked: boolean;
  selected: number;
}) {
  const [specs, setSpecs] = useState<ParamSpecView[]>([]);

  useEffect(() => {
    listBotParams().then(setSpecs).catch(() => setSpecs([]));
  }, []);

  const seat = Math.min(selected, request.players - 1);
  // 座位列表尚未補齊（座位數剛改變的那一個 render）時仍要能畫，
  // 否則面板會閃一下空白
  const current = request.bots[seat] ?? { name: `座位 ${seat}`, params: {} };
  const isHero = seat === request.heroSeat;

  const updateSeat = (next: typeof current) =>
    onChange({
      ...request,
      bots: Array.from({ length: request.players }, (_, index) =>
        index === seat ? next : (request.bots[index] ?? { name: `座位 ${index}`, params: {} }),
      ),
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
  const changed = Object.keys(current.params).length;

  return (
    <div style={{ padding: 20, maxWidth: 1000 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          gap: 16,
          marginBottom: 4,
        }}
      >
        <h2 style={{ fontSize: 15, margin: 0 }}>
          座位 {seat}
          <span className="dim" style={{ marginLeft: 8, fontSize: 12 }}>
            {current.name || '未命名'}
          </span>
          {isHero && (
            <span style={{ color: 'var(--accent)', marginLeft: 6, fontSize: 12 }}>· 你</span>
          )}
        </h2>
        <span className="dim" style={{ fontSize: 11 }}>
          {changed > 0 ? `已調整 ${changed} 項` : '全部為預設值'}
        </span>
      </div>
      <p className="dim" style={{ fontSize: 11, margin: '0 0 16px' }}>
        參數的鍵、範圍與說明由引擎提供，前端不自行抄一份。執行期間鎖定。
      </p>

      {specs.length === 0 && (
        <div
          style={{
            padding: '8px 10px',
            marginBottom: 16,
            border: '1px solid var(--warning)',
            borderRadius: 'var(--radius-control)',
            color: 'var(--warning)',
            fontSize: 11,
          }}
        >
          參數規格由引擎提供，瀏覽器模式取不到。請用桌面版。
        </div>
      )}

      <section style={{ ...cardStyle, marginBottom: 16 }}>
        <div style={{ display: 'flex', gap: 12, alignItems: 'flex-end' }}>
          <div style={{ flex: 1, maxWidth: 260 }}>
            <div className="dim" style={{ fontSize: 11, marginBottom: 4 }}>
              名稱
            </div>
            <TextInput
              value={current.name}
              disabled={locked}
              onChange={(name) => updateSeat({ ...current, name })}
            />
          </div>
          <button
            type="button"
            disabled={locked || changed === 0}
            onClick={() => updateSeat({ ...current, params: {} })}
            style={{
              padding: '6px 10px',
              borderRadius: 'var(--radius-control)',
              border: '1px solid var(--border)',
              background: 'transparent',
              color: 'var(--text-secondary)',
              fontSize: 11,
              fontFamily: 'inherit',
              cursor: locked || changed === 0 ? 'default' : 'pointer',
              opacity: locked || changed === 0 ? 0.4 : 1,
            }}
          >
            全部復原為預設
          </button>
        </div>
      </section>

      <ParamGroup
        title="人格層"
        specs={persona}
        params={current.params}
        onChange={setParam}
        locked={locked}
      />
      {/* 四支人格滑桿在走表的節點上是靠內容層位移作用，光看數字看不出
          它們把哪幾手牌搬去哪裡。把「照表打」與目前這組參數疊起來比 */}
      <section style={{ ...cardStyle, marginBottom: 16 }}>
        <SectionTitle>範圍預覽</SectionTitle>
        <div className="dim" style={{ fontSize: 11, marginBottom: 10, lineHeight: 1.6 }}>
          這個座位在選定節點會打出的範圍。外框標出的是被這組參數改掉的格子，
          底色仍代表現在打什麼。範圍由引擎算，走的是 Bot 實際決策時的同一支推導，
          因此這裡畫的就是它會打的東西。
        </div>
        <BotRangePreview seated={request.players} bot={current} />
      </section>

      <ParamGroup
        title="行為層"
        specs={behavior}
        params={current.params}
        onChange={setParam}
        locked={locked}
      />

      {pending.length > 0 && (
        <section style={{ ...cardStyle, marginBottom: 16 }}>
          <SectionTitle>尚未生效（{pending.length}）</SectionTitle>
          <div className="dim" style={{ fontSize: 11, marginBottom: 10, lineHeight: 1.6 }}>
            這些欄位已在核心規格 4.3 宣告，但決策路徑目前不會讀到，
            調了不會改變任何結果，因此停用而不是讓你白拉。
          </div>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))',
              gap: '2px 20px',
            }}
          >
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
          </div>
        </section>
      )}
    </div>
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
    <section style={{ ...cardStyle, marginBottom: 16 }}>
      <SectionTitle>
        {title}（{specs.length}）
      </SectionTitle>
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
          gap: '4px 24px',
        }}
      >
        {specs.map((spec) => (
          <ParamField
            key={spec.key}
            spec={spec}
            value={params[spec.key]}
            onChange={(value) => onChange(spec.key, value)}
            locked={locked}
          />
        ))}
      </div>
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
          {isMyriad ? `${spec.min / 100}–${spec.max / 100}%` : `${spec.min}–${spec.max}`}
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
              {
                flex: 1,
                '--range-accent': changed ? 'var(--warning)' : 'var(--accent)',
              } as React.CSSProperties
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
        margin: '0 0 10px',
        textTransform: 'uppercase',
        letterSpacing: '0.05em',
      }}
    >
      {children}
    </h3>
  );
}

const cardStyle: React.CSSProperties = {
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-panel)',
  background: 'var(--bg-surface)',
  padding: 16,
};
