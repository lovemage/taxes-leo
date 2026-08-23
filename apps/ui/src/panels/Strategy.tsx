// 面板 D — 自身策略（右側資訊窗）。
//
// UI 規格 D.1：資訊密度最高的面板，編輯「自身座位」的策略。節點在左欄
// 選，這裡畫該節點的 169 格範圍矩陣並提供逐格編輯。
//
// # 只有翻前
//
// 翻前走參數化 baseline，位置、籌碼分檔與情境真的會改變分佈，攤開來
// 是有內容的。**翻後沒有內容**——顧問的規則表還沒進來，一律走 fallback。
// 因此這裡不畫 D.5 的翻後規則清單，只如實標示 fallback 版本：畫一個空的
// 規則編輯器只會讓人以為那裡有策略。
//
// # 頻率一律由引擎算
//
// 這個檔案不重算任何頻率、寬度或加注尺度。使用者改一格，前端做的只是
// 把「這一格改成多少」送回引擎，再把引擎重算的矩陣畫出來。UI 自己算的
// 話，面板顯示的範圍會與 Bot 實際打的漂移，而且完全沒有徵兆。

import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  CellOverrideView,
  MatrixCellView,
  RangeMatrixView,
  StrategyMetaView,
} from '../../../../packages/poker-types/src/index';
import { strategyMatrix, strategyMeta } from '../api';
import type { StrategySelection } from './StrategyNav';

/** 頻率的滿值。跨 IPC 一律萬分比整數 */
const FULL = 10_000;

export function Strategy({
  selection,
  overrides,
  onOverridesChange,
  locked,
}: {
  selection: StrategySelection;
  overrides: CellOverrideView[];
  onOverridesChange: (overrides: CellOverrideView[]) => void;
  /** run 進行中不得修改策略：內容是 RunManifest 快照的一部分 */
  locked: boolean;
}) {
  const [meta, setMeta] = useState<StrategyMetaView | null>(null);
  const [matrix, setMatrix] = useState<RangeMatrixView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [pending, setPending] = useState(true);

  useEffect(() => {
    strategyMeta().then(setMeta).catch(() => setMeta(null));
  }, []);

  // 回應可能亂序抵達（第一次請求要等 equity 排序算完，後續是即時的）。
  // 只採用最後一次發出的請求，否則放開滑桿後畫面會跳回舊的矩陣
  const issued = useRef(0);

  useEffect(() => {
    const ticket = ++issued.current;
    setPending(true);
    strategyMatrix(
      selection.seated,
      selection.hero,
      selection.bucket,
      selection.scenario,
      overrides,
    )
      .then((view) => {
        if (ticket !== issued.current) return;
        setMatrix(view);
        setFailure(null);
        setPending(false);
      })
      .catch((error: unknown) => {
        if (ticket !== issued.current) return;
        setFailure(String(error));
        setPending(false);
      });
  }, [selection, overrides]);

  const isThisNode = useCallback(
    (item: CellOverrideView) =>
      item.seated === selection.seated &&
      item.hero === selection.hero &&
      item.bucket === selection.bucket &&
      item.scenario === selection.scenario,
    [selection],
  );

  const setCell = (className: string, aggressive: number, call: number) => {
    const rest = overrides.filter((item) => !(isThisNode(item) && item.class === className));
    onOverridesChange([
      ...rest,
      {
        seated: selection.seated,
        hero: selection.hero,
        bucket: selection.bucket,
        scenario: selection.scenario,
        class: className,
        aggressive,
        call,
      },
    ]);
  };

  const clearCell = (className: string) =>
    onOverridesChange(overrides.filter((item) => !(isThisNode(item) && item.class === className)));

  const clearNode = () => onOverridesChange(overrides.filter((item) => !isThisNode(item)));

  const cell = matrix?.cells.find((item) => item.class === picked) ?? null;

  return (
    <div style={{ padding: 20 }}>
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
          自身策略
          <span className="dim" style={{ marginLeft: 8, fontSize: 12 }}>
            {matrix ? `${matrix.seated}-max ${matrix.hero}｜${matrix.scenarioLabel}` : '載入中'}
          </span>
        </h2>
        <span className="dim" style={{ fontSize: 11, fontFamily: 'var(--font-mono)' }}>
          {matrix?.nodeKey ?? ''}
        </span>
      </div>
      <p className="dim" style={{ fontSize: 11, margin: '0 0 16px', lineHeight: 1.6 }}>
        節點在左欄選。點任一格可改該手牌的行動頻率；改過的格會蓋掉參數產生的結果，
        並隨 run 寫進 RunManifest 快照。
      </p>

      {failure && <Banner tone="negative">{failure}</Banner>}
      {locked && <Banner tone="warning">run 進行中，策略鎖定。</Banner>}
      {meta && !meta.consultantApproved && (
        <Banner tone="warning">
          目前的 {meta.baselineName}（{meta.baselineVersion}）
          <strong>尚未經牌手顧問簽核</strong>，是參數化產生的工程佔位內容。
          可以拿來驗證管線，不得當成校準過的策略解讀。
        </Banner>
      )}

      {!matrix && !failure && (
        <section style={{ ...cardStyle, maxWidth: 420 }}>
          <SectionTitle>載入中</SectionTitle>
          <div className="dim" style={{ fontSize: 11, lineHeight: 1.6 }}>
            首次開啟要先建立 equity 排序（20,000 次取樣 × 4 種對手數），約需數秒。
            排序全程只算一次，之後切換節點是即時的。
          </div>
        </section>
      )}

      {matrix && (
        <div style={{ display: 'flex', gap: 18, alignItems: 'flex-start', flexWrap: 'wrap' }}>
          {/* ── D.4 169 格範圍矩陣 ── */}
          <section style={{ flex: '1 1 560px', minWidth: 420, maxWidth: 720 }}>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(120px, 1fr))',
                gap: '6px 16px',
                marginBottom: 12,
              }}
            >
              <Stat label="範圍寬度" value={`${(matrix.widthMyriad / 100).toFixed(1)}%`} />
              <Stat label="混合格" value={`${matrix.mixedCount} 格`} />
              <Stat label="主動行動" value={matrix.aggressiveAction} />
              <Stat label="排序對手數" value={`${matrix.expectedOpponents} 人`} />
            </div>

            <div
              className="dim"
              style={{ display: 'flex', gap: 14, flexWrap: 'wrap', fontSize: 11, marginBottom: 8 }}
            >
              <Swatch color="var(--matrix-aggressive)">100% 主動</Swatch>
              <Swatch color="rgba(var(--matrix-mix-rgb), 0.45)">混合</Swatch>
              <Swatch color="var(--matrix-call)">跟注</Swatch>
              <Swatch color="var(--matrix-empty)">棄牌</Swatch>
              <span>寬度以 combo 加權，非 169 類等權</span>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(13, minmax(0, 1fr))',
                gap: 1,
                opacity: pending ? 0.55 : 1,
                transition: 'opacity 120ms linear',
              }}
            >
              {matrix.cells.map((item) => (
                <Cell
                  key={item.class}
                  cell={item}
                  picked={item.class === picked}
                  onClick={() => setPicked(item.class)}
                />
              ))}
            </div>

            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'baseline',
                marginTop: 8,
                fontSize: 11,
              }}
            >
              <span className="dim">
                對角線為對子，右上為同花，左下為非同花（共 169 類）
              </span>
              {matrix.overrideCount > 0 && (
                <button
                  type="button"
                  disabled={locked}
                  onClick={clearNode}
                  style={linkStyle(locked)}
                >
                  清除本節點的 {matrix.overrideCount} 格覆寫
                </button>
              )}
            </div>
          </section>

          {/* ── 選定格的編輯區 ── */}
          <aside style={{ flex: '0 1 300px', minWidth: 260, display: 'grid', gap: 14 }}>
            <CellEditor
              cell={cell}
              locked={locked}
              onSet={(aggressive, call) => cell && setCell(cell.class, aggressive, call)}
              onClear={() => cell && clearCell(cell.class)}
            />
            {meta && <ContentCard meta={meta} />}
          </aside>
        </div>
      )}
    </div>
  );
}

/**
 * 一格。
 *
 * 顏色沿用顧問校準工作台：100% 主動為深綠、混合以綠色透明度表示比例、
 * 純跟注為藍、其餘為底色。同一份範圍在兩個工具裡換色，對照時會以為
 * 看到的是不同內容。
 */
function Cell({
  cell,
  picked,
  onClick,
}: {
  cell: MatrixCellView;
  picked: boolean;
  onClick: () => void;
}) {
  const tone = cellTone(cell);
  return (
    <button
      type="button"
      onClick={onClick}
      title={`${cell.class}｜主動 ${pct(cell.aggressive)}／跟注 ${pct(cell.call)}／棄牌 ${pct(
        cell.fold,
      )}`}
      style={{
        position: 'relative',
        aspectRatio: '1',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 1,
        border: 'none',
        borderRadius: 0,
        outline: picked ? '2px solid var(--text-primary)' : 'none',
        outlineOffset: -2,
        fontFamily: 'var(--font-mono)',
        fontSize: 10,
        lineHeight: 1,
        cursor: 'pointer',
        ...tone,
      }}
    >
      <span>{cell.class}</span>
      {cell.aggressive > 0 && cell.aggressive < FULL && (
        <span style={{ fontSize: 8, opacity: 0.85 }}>{pct(cell.aggressive)}</span>
      )}
      {cell.overridden && (
        <span
          style={{
            position: 'absolute',
            top: 2,
            right: 2,
            width: 4,
            height: 4,
            background: 'var(--warning)',
          }}
        />
      )}
    </button>
  );
}

function cellTone(cell: MatrixCellView): React.CSSProperties {
  if (cell.aggressive >= FULL) {
    return { background: 'var(--matrix-aggressive)', color: 'var(--text-primary)' };
  }
  if (cell.aggressive > 0) {
    const ratio = ((cell.aggressive / FULL) * 0.77).toFixed(2);
    return { background: `rgba(var(--matrix-mix-rgb), ${ratio})`, color: 'var(--bg-base)' };
  }
  if (cell.call > 0) return { background: 'var(--matrix-call)', color: 'var(--text-primary)' };
  return { background: 'var(--matrix-empty)', color: 'var(--text-tertiary)' };
}

/**
 * 逐格編輯。
 *
 * 只輸入主動與跟注兩個數字，**棄牌是餘數**——引擎的 `OverrideCell` 就是
 * 這個形狀，因此不可能寫出合計不等於 100% 的覆寫（D.8 的頻率合計檢查
 * 在型別層就過了，不必等到儲存前才驗）。
 */
function CellEditor({
  cell,
  locked,
  onSet,
  onClear,
}: {
  cell: MatrixCellView | null;
  locked: boolean;
  onSet: (aggressive: number, call: number) => void;
  onClear: () => void;
}) {
  if (!cell) {
    return (
      <section style={cardStyle}>
        <SectionTitle>逐格編輯</SectionTitle>
        <div className="dim" style={{ fontSize: 11, lineHeight: 1.6 }}>
          點矩陣裡的任一格開始編輯。改一格只影響這個節點的這一類手牌，
          不會連帶動到相鄰牌類或其他節點。
        </div>
      </section>
    );
  }

  // 主動加跟注不得超過 100%：先動的那個保留，另一個讓位
  const setAggressive = (next: number) => onSet(next, Math.min(cell.call, FULL - next));
  const setCall = (next: number) => onSet(Math.min(cell.aggressive, FULL - next), next);

  return (
    <section style={cardStyle}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 8,
        }}
      >
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 16 }}>{cell.class}</span>
        <span className="dim" style={{ fontSize: 10 }}>
          {cell.combos} combo · equity 前 {pct(cell.percentile)}
        </span>
      </div>

      <Freq label="主動" value={cell.aggressive} locked={locked} onChange={setAggressive} />
      <Freq label="跟注" value={cell.call} locked={locked} onChange={setCall} />
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          fontSize: 11,
          padding: '4px 0 8px',
          color: 'var(--text-secondary)',
        }}
      >
        <span>棄牌（餘數）</span>
        <span className="num">{pct(cell.fold)}</span>
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
        <Chip disabled={locked} onClick={() => onSet(FULL, 0)}>
          全主動
        </Chip>
        <Chip disabled={locked} onClick={() => onSet(0, FULL)}>
          全跟注
        </Chip>
        <Chip disabled={locked} onClick={() => onSet(0, 0)}>
          全棄牌
        </Chip>
        {cell.overridden && (
          <Chip disabled={locked} onClick={onClear}>
            清除覆寫
          </Chip>
        )}
      </div>

      <div className="dim" style={{ fontSize: 10, marginTop: 8, lineHeight: 1.5 }}>
        {cell.overridden
          ? '這一格是你的覆寫，參數不再影響它。'
          : '目前由參數產生。改動後即成為覆寫。'}
      </div>
    </section>
  );
}

function Freq({
  label,
  value,
  locked,
  onChange,
}: {
  label: string;
  value: number;
  locked: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div style={{ marginBottom: 8 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          fontSize: 11,
          marginBottom: 3,
        }}
      >
        <span>{label}</span>
        <span className="num">{pct(value)}</span>
      </div>
      <input
        type="range"
        min={0}
        max={FULL}
        step={100}
        value={value}
        disabled={locked}
        onChange={(e) => onChange(Number(e.target.value))}
        style={{ width: '100%' }}
      />
    </div>
  );
}

/** D.6／D.7：內容來源、加注尺度與翻後 fallback，如實揭露 */
function ContentCard({ meta }: { meta: StrategyMetaView }) {
  return (
    <section style={cardStyle}>
      <SectionTitle>內容現況</SectionTitle>
      <Row label="基準" value={`${meta.baselineName}`} />
      <Row label="版本" value={meta.baselineVersion} />
      <Row label="顧問簽核" value={meta.consultantApproved ? '已簽核' : '未簽核'} />
      <Row label="翻前節點" value={`${meta.preflopNodeCount.toLocaleString()} 個`} />
      <Row label="翻前格數" value={`${meta.preflopCellCount.toLocaleString()} 格`} />
      <Row label="開牌尺度" value={`${(meta.openSizeCentiBb / 100).toFixed(2)} BB`} />
      <Row label="3-bet 尺度" value={`${(meta.threeBetSizeCentiBb / 100).toFixed(2)} BB`} />
      <Row label="4-bet 尺度" value={`${(meta.fourBetSizeCentiBb / 100).toFixed(2)} BB`} />
      <Row label="推入門檻" value={`${meta.pushFoldBelow} BB 以下`} />
      <Row label="equity 取樣" value={meta.rankingSamples.toLocaleString()} />
      <div
        style={{
          marginTop: 10,
          paddingTop: 10,
          borderTop: '1px solid var(--border)',
          fontSize: 11,
          lineHeight: 1.6,
          color: 'var(--text-secondary)',
        }}
      >
        <div style={{ marginBottom: 4 }}>
          翻後一律 fallback（
          <span style={{ fontFamily: 'var(--font-mono)' }}>{meta.postflopFallback}</span>）。
        </div>
        <span className="dim">
          顧問的翻後規則表尚未進來。翻後完整度 0%，UI 規格 D.5 的規則清單
          要等有內容才畫得出來。
        </span>
      </div>
    </section>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'baseline',
        gap: 12,
        fontSize: 11,
        padding: '2px 0',
      }}
    >
      <span className="dim">{label}</span>
      <span className="num" style={{ color: 'var(--text-secondary)' }}>
        {value}
      </span>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="dim" style={{ fontSize: 10 }}>
        {label}
      </div>
      <div style={{ fontSize: 13, fontFamily: 'var(--font-mono)' }}>{value}</div>
    </div>
  );
}

function Swatch({ color, children }: { color: string; children: React.ReactNode }) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
      <span
        style={{
          width: 12,
          height: 12,
          background: color,
          border: '1px solid var(--border)',
          display: 'inline-block',
        }}
      />
      {children}
    </span>
  );
}

function Chip({
  disabled,
  onClick,
  children,
}: {
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      style={{
        padding: '4px 8px',
        borderRadius: 'var(--radius-chip)',
        border: '1px solid var(--border)',
        background: 'transparent',
        color: 'var(--text-secondary)',
        fontSize: 11,
        fontFamily: 'inherit',
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
      }}
    >
      {children}
    </button>
  );
}

function Banner({ tone, children }: { tone: 'warning' | 'negative'; children: React.ReactNode }) {
  const color = tone === 'warning' ? 'var(--warning)' : 'var(--negative)';
  return (
    <div
      style={{
        border: `1px solid ${color}`,
        borderRadius: 'var(--radius-control)',
        color,
        padding: '8px 12px',
        marginBottom: 12,
        fontSize: 12,
        lineHeight: 1.6,
        maxWidth: 720,
      }}
    >
      {children}
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

function linkStyle(locked: boolean): React.CSSProperties {
  return {
    padding: 0,
    border: 'none',
    background: 'none',
    color: 'var(--accent)',
    fontSize: 11,
    fontFamily: 'inherit',
    cursor: locked ? 'default' : 'pointer',
    opacity: locked ? 0.4 : 1,
  };
}

function pct(myriad: number): string {
  return `${(myriad / 100).toFixed(1)}%`;
}

const cardStyle: React.CSSProperties = {
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-panel)',
  background: 'var(--bg-surface)',
  padding: 16,
};
