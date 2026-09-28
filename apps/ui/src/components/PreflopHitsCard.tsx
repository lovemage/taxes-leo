// 翻前情境命中（執行快照 `preflopHits`）。
//
// 策略頁列的是「可能發生的情境」，這裡是上一次 run 裡英雄**實際遇到**的
// 情境：3-bet 與擠壓各幾次、對手開了哪些 OPEN 尺寸、各落在哪個區間。
// 標籤與排序由引擎給，前端不拆情境鍵。

import { useEffect, useState } from 'react';
import type { PreflopHitsView } from '../../../../packages/poker-types/src/index';
import { getRun } from '../api';

export function PreflopHitsCard({ reloadToken }: { reloadToken: number }) {
  const [hits, setHits] = useState<PreflopHitsView | null>(null);
  const [runId, setRunId] = useState<number | null>(null);
  /** 讀取失敗。與「舊版 run 沒有這份資料」分開，否則暫時性錯誤會被說成舊版 */
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getRun()
      .then((run) => {
        if (!active) return;
        setRunId(run.runId);
        setHits(run.preflopHits);
        setFailure(null);
      })
      // 保留上一次成功讀到的資料，只標示這次沒讀到
      .catch((error: unknown) => {
        if (active) setFailure(String(error));
      });
    return () => {
      active = false;
    };
  }, [reloadToken]);

  if (runId === null) {
    return failure ? (
      <section id="preflop-hits" className="preflop-hits">
        <h3>翻前情境命中</h3>
        <p style={{ color: 'var(--negative)' }}>讀取 run 資料失敗：{failure}</p>
      </section>
    ) : null;
  }

  if (!hits) {
    return (
      <section id="preflop-hits" className="preflop-hits">
        <h3>翻前情境命中</h3>
        <p className="dim">這個 run 是 0.1.7 以前建立的，快照裡沒有翻前情境命中紀錄。</p>
      </section>
    );
  }

  const groups: Array<{ kind: string; label: string; total: number; rows: PreflopHitsView['scenarios'] }> = [];
  for (const row of hits.scenarios) {
    const last = groups[groups.length - 1];
    if (last && last.kind === row.kind) {
      last.rows.push(row);
      last.total += row.count;
    } else {
      groups.push({ kind: row.kind, label: row.kindLabel, total: row.count, rows: [row] });
    }
  }
  const share = (count: number) =>
    hits.total === 0 ? '0.0%' : `${((count / hits.total) * 100).toFixed(1)}%`;
  const bb = (centi: number) => `${Number((centi / 100).toFixed(2))} BB`;

  const tiers = ['standard', 'medium', 'large'].map((tier) => {
    const sizes = hits.openSizes.filter((hit) => hit.tier === tier);
    return {
      tier,
      label: sizes[0]?.tierLabel ?? TIER_LABEL[tier],
      range:
        tier === 'standard'
          ? `≤ ${bb(hits.mediumAboveCentiBb)}`
          : tier === 'medium'
            ? `> ${bb(hits.mediumAboveCentiBb)}～${bb(hits.largeAboveCentiBb)}`
            : `> ${bb(hits.largeAboveCentiBb)}`,
      total: sizes.reduce((sum, hit) => sum + hit.count, 0),
      sizes,
    };
  });

  return (
    <section id="preflop-hits" className="preflop-hits">
      <div className="preflop-hits__heading">
        <h3>翻前情境命中 · run #{runId}</h3>
        <span className="dim num">Hero 翻前決策 {hits.total.toLocaleString()} 次</span>
      </div>
      {failure && (
        <p style={{ color: 'var(--warning)', margin: '0 0 8px' }}>
          最新資料讀取失敗，以下仍是上一次讀到的內容：{failure}
        </p>
      )}

      <div className="preflop-hits__body">
        <div className="preflop-hits__groups">
          {groups.map((group) => (
            <div key={group.kind} className="preflop-hits__group">
              <div className="preflop-hits__group-title">
                <strong>{group.label}</strong>
                <span className="num">
                  {group.total.toLocaleString()}（{share(group.total)}）
                </span>
              </div>
              <div className="preflop-hits__rows">
                {group.rows.map((row) => (
                  <span key={row.key} className="preflop-hits__row" title={row.key}>
                    {row.label}
                    <span className="num">{row.count.toLocaleString()}</span>
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className="preflop-hits__open">
          <div className="preflop-hits__group-title">
            <strong>面對 OPEN 的實際尺寸</strong>
          </div>
          {tiers.map((tier) => (
            <div key={tier.tier} className="preflop-hits__tier">
              <div className="preflop-hits__tier-title">
                <span>
                  {tier.label} <span className="dim num">{tier.range}</span>
                </span>
                <span className="num">{tier.total.toLocaleString()}</span>
              </div>
              <div className="dim num preflop-hits__sizes">
                {tier.sizes.length === 0
                  ? '未遇到'
                  : tier.sizes.map((hit) => `${bb(hit.centiBb)} ×${hit.count}`).join('　')}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

const TIER_LABEL: Record<string, string> = {
  standard: '標準 OPEN',
  medium: '中型 OPEN',
  large: '大型 OPEN',
};
