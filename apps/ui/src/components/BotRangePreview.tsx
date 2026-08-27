// 面板 C 的即時範圍預覽。
//
// 四支人格滑桿（rangeWidth／preflopAggression／callPersistence／
// foldDiscipline）在走預設組合表的節點上是靠內容層位移作用，光看參數
// 數字看不出它們把哪幾手牌搬去哪裡。這裡把「照表打」與「這組參數」
// 兩張矩陣疊起來比，改動過的格子標出來。
//
// 範圍本身一律由引擎算，而且走的是 Bot 實際決策時的同一支推導
// （`poker_engine::bot::rules_for_bot`）。前端自己算的話，預覽畫的就
// 不是 Bot 會打的東西——而那是這個元件存在的唯一理由。

import { useEffect, useRef, useState } from 'react';
import type {
  BotSeatConfig,
  MatrixCellView,
  RangeMatrixView,
  StrategyNodesView,
} from '../../../../packages/poker-types/src/index';
import { botStrategyMatrix, strategyNodes } from '../api';
import { ACTION_LABEL, cellTone, dominantAction, FULL } from './matrixTone';

/** 一格的變化：從哪個動作變成哪個動作 */
interface Move {
  class: string;
  combos: number;
  from: string;
  to: string;
}

export function BotRangePreview({ seated, bot }: { seated: number; bot: BotSeatConfig }) {
  const [nav, setNav] = useState<StrategyNodesView | null>(null);
  const [hero, setHero] = useState('BTN');
  const [scenario, setScenario] = useState('unopened');
  const [bucket, setBucket] = useState('160-240');
  const [neutral, setNeutral] = useState<RangeMatrixView | null>(null);
  const [current, setCurrent] = useState<RangeMatrixView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    strategyNodes(seated, hero)
      .then((view) => {
        setNav(view);
        // 呼叫端給的位置不屬於該桌型時引擎會落到第一個，跟著它走
        if (view.hero !== hero) setHero(view.hero);
        if (!view.scenarios.some((item) => item.key === scenario)) {
          setScenario(view.scenarios[0]?.key ?? 'unopened');
        }
        if (!view.buckets.some((item) => item.key === bucket)) {
          setBucket(view.buckets[0]?.key ?? '160-240');
        }
      })
      .catch(() => setNav(null));
    // scenario／bucket 只在清單不含目前值時被修正，不必進相依
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seated, hero]);

  // 照表打的基準。只隨節點變，與參數無關
  useEffect(() => {
    botStrategyMatrix(seated, hero, bucket, scenario, { name: '照表', params: {} })
      .then(setNeutral)
      .catch((e: unknown) => setFailure(String(e)));
  }, [seated, hero, bucket, scenario]);

  // `bot` 在父層是 `request.bots[seat] ?? {...}`，每次 render 都是新物件；
  // 直接放進相依會無限發請求。改用序列化後的內容當鍵，實際值走 ref
  const botRef = useRef(bot);
  botRef.current = bot;
  const botKey = JSON.stringify([bot.name, bot.params]);

  // 回應可能亂序抵達——拖滑桿會連發好幾次請求。只採用最後一次發出的，
  // 否則放開滑桿後畫面會跳回中途某一格的結果
  const latest = useRef(0);
  useEffect(() => {
    const ticket = ++latest.current;
    // 拖曳中每一個中間值都送一次請求沒有意義：畫面來不及看，引擎白算
    const timer = window.setTimeout(() => {
      botStrategyMatrix(seated, hero, bucket, scenario, botRef.current)
        .then((view) => {
          if (ticket === latest.current) {
            setCurrent(view);
            setFailure(null);
          }
        })
        .catch((e: unknown) => {
          if (ticket === latest.current) setFailure(String(e));
        });
    }, 120);
    return () => window.clearTimeout(timer);
  }, [seated, hero, bucket, scenario, botKey]);

  if (failure) {
    return (
      <p className="dim" style={{ fontSize: 11 }}>
        預覽不可用：{failure}
      </p>
    );
  }
  if (!current || !neutral) {
    return (
      <p className="dim" style={{ fontSize: 11 }}>
        載入預覽…
      </p>
    );
  }

  const moves = diff(neutral.cells, current.cells);
  const movedCombos = moves.reduce((sum, move) => sum + move.combos, 0);

  return (
    <div>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 10 }}>
        <Select value={hero} onChange={setHero} options={nav?.positions ?? [hero]} />
        <Select
          value={scenario}
          onChange={setScenario}
          options={(nav?.scenarios ?? []).map((item) => item.key)}
          labels={Object.fromEntries((nav?.scenarios ?? []).map((i) => [i.key, i.label]))}
        />
        <Select
          value={bucket}
          onChange={setBucket}
          options={(nav?.buckets ?? []).map((item) => item.key)}
          labels={Object.fromEntries((nav?.buckets ?? []).map((i) => [i.key, i.label]))}
        />
      </div>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(13, minmax(0, 1fr))',
          gap: 1,
          maxWidth: 360,
        }}
      >
        {current.cells.map((cell, index) => {
          const before = neutral.cells[index];
          const moved = before !== undefined && dominantAction(before) !== dominantAction(cell);
          return (
            <span
              key={cell.class}
              title={
                moved && before
                  ? `${cell.class}：${ACTION_LABEL[dominantAction(before)]} → ${ACTION_LABEL[dominantAction(cell)]}`
                  : `${cell.class}：${ACTION_LABEL[dominantAction(cell)]}（照表）`
              }
              style={{
                aspectRatio: '1',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontFamily: 'var(--font-mono)',
                fontSize: 8,
                lineHeight: 1,
                // 變動的格子用外框標，不換底色——底色仍要說明「現在打什麼」
                outline: moved ? '2px solid var(--warning)' : 'none',
                outlineOffset: -2,
                ...cellTone(cell),
              }}
            >
              {cell.class}
            </span>
          );
        })}
      </div>

      <div style={{ marginTop: 10, fontSize: 11 }}>
        <div>
          {/* 「寬度」在這裡與面板 D 同義：combo 加權的**主動**頻率，不是進池率。
              侵略性把跟注改成加注時它會變大，即使進池的牌變少了 */}
          <span title="以 combo 加權的主動（加注）頻率，不是進池率">範圍寬度</span>{' '}
          <span className="num" style={{ color: 'var(--text-primary)' }}>
            {pct(neutral.widthMyriad)} → {pct(current.widthMyriad)}
          </span>
          <span className="dim" style={{ marginLeft: 8 }}>
            {current.source === 'chart' ? '本節點走顧問的預設組合表' : '本節點走參數產生器'}
          </span>
        </div>

        {moves.length === 0 ? (
          <p className="dim" style={{ margin: '6px 0 0', lineHeight: 1.6 }}>
            這組參數在這個節點上與照表打完全相同。
            {Object.keys(bot.params).length > 0 && current.cells.every((c) => c.call === 0) && (
              <>
                <br />
                這個節點只有加注與棄牌，沒有跟注帶可以搬——跟注黏著度與棄牌紀律
                在這裡本來就不會作用。換到「面對開牌」之類的情境才看得出來。
              </>
            )}
          </p>
        ) : (
          <p style={{ margin: '6px 0 0', color: 'var(--warning)' }}>
            {moves.length} 類手牌（{movedCombos} combo）換了動作：{summarize(moves)}
          </p>
        )}
      </div>
    </div>
  );
}

/** 逐格比對主導動作。頻率的微幅變化不算——那看不出來也講不清楚 */
function diff(before: MatrixCellView[], after: MatrixCellView[]): Move[] {
  const moves: Move[] = [];
  after.forEach((cell, index) => {
    const was = before[index];
    if (!was) return;
    const from = dominantAction(was);
    const to = dominantAction(cell);
    if (from !== to) {
      moves.push({ class: cell.class, combos: cell.combos, from, to });
    }
  });
  return moves;
}

/** 「跟注 → 加注 8 類」這種說法。逐格列出 169 類沒有人看得完 */
function summarize(moves: Move[]): string {
  const grouped = new Map<string, number>();
  moves.forEach((move) => {
    const key = `${ACTION_LABEL[move.from]} → ${ACTION_LABEL[move.to]}`;
    grouped.set(key, (grouped.get(key) ?? 0) + 1);
  });
  return [...grouped.entries()].map(([key, count]) => `${key} ${count} 類`).join('、');
}

function pct(myriad: number): string {
  return `${((myriad / FULL) * 100).toFixed(1)}%`;
}

function Select({
  value,
  onChange,
  options,
  labels,
}: {
  value: string;
  onChange: (next: string) => void;
  options: string[];
  labels?: Record<string, string>;
}) {
  return (
    <select
      value={value}
      onChange={(event) => onChange(event.target.value)}
      style={{
        padding: '3px 6px',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius-control)',
        background: 'var(--bg-raised)',
        color: 'var(--text-primary)',
        fontFamily: 'inherit',
        fontSize: 11,
      }}
    >
      {options.map((option) => (
        <option key={option} value={option}>
          {labels?.[option] ?? option}
        </option>
      ))}
    </select>
  );
}
