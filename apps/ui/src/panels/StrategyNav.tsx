// 面板 D — 情境導航（左欄）。
//
// UI 規格 D.1 的版面：左側情境導航樹（桌型 → 位置 → 情境 → 籌碼分檔），
// 右側編輯區。這一欄只決定「看哪個節點」，範圍矩陣本身在右側資訊窗。
//
// 清單一律由引擎列舉（`strategyNodes`），前端不自己算哪些情境到得了。
// 自己算的話遲早會列出 UTG「面對開牌」這種到不了的節點，使用者會在
// 一個永遠不會被查到的格子上編輯。

import { useEffect, useState } from 'react';
import type { StrategyNodesView } from '../../../../packages/poker-types/src/index';
import { strategyNodes } from '../api';
import { Segmented } from '../components/Field';

/** 目前檢視中的翻前節點。欄位鍵與引擎決策時查表用的完全相同 */
export interface StrategySelection {
  seated: number;
  hero: string;
  bucket: string;
  scenario: string;
}

export const DEFAULT_SELECTION: StrategySelection = {
  seated: 9,
  hero: 'BTN',
  bucket: '160-240',
  scenario: 'unopened',
};

export function StrategyNav({
  selection,
  onChange,
  overrideCount,
}: {
  selection: StrategySelection;
  onChange: (selection: StrategySelection) => void;
  /** 全部節點的覆寫筆數，供左欄一眼看出自己改了多少 */
  overrideCount: number;
}) {
  const [nodes, setNodes] = useState<StrategyNodesView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    strategyNodes(selection.seated, selection.hero)
      .then((view) => {
        if (disposed) return;
        setNodes(view);
        setFailure(null);
        // 桌型換小之後，原本的位置或情境可能不存在（9 人桌的 UTG+2 在
        // 6 人桌沒有）。引擎回的是修正後的結果，這裡同步回選取狀態，
        // 否則左欄顯示的節點與右側矩陣畫的會是兩件事
        const scenarioOk = view.scenarios.some((item) => item.key === selection.scenario);
        const nextScenario = scenarioOk ? selection.scenario : 'unopened';
        if (view.hero !== selection.hero || nextScenario !== selection.scenario) {
          onChange({ ...selection, hero: view.hero, scenario: nextScenario });
        }
      })
      .catch((error: unknown) => {
        if (!disposed) setFailure(String(error));
      });
    return () => {
      disposed = true;
    };
  }, [selection, onChange]);

  const groups = groupScenarios(nodes);

  return (
    <>
      {failure && (
        <div
          style={{
            padding: '8px 10px',
            marginBottom: 12,
            border: '1px solid var(--warning)',
            borderRadius: 'var(--radius-control)',
            color: 'var(--warning)',
            fontSize: 11,
            lineHeight: 1.5,
          }}
        >
          策略內容由引擎提供，目前取不到。桌面版請確認引擎已啟動，
          瀏覽器模式請確認 dev server 在跑。
        </div>
      )}

      <section style={{ marginBottom: 18 }}>
        <SectionTitle>桌型</SectionTitle>
        <Segmented
          value={selection.seated}
          options={[6, 7, 8, 9]}
          onChange={(seated) => onChange({ ...selection, seated })}
        />
        <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
          破產離桌會讓在桌人數在同一個 run 內下降，因此四種桌型都要看過
          （UI 規格 D.3）。
        </div>
      </section>

      <section style={{ marginBottom: 18 }}>
        <SectionTitle>位置</SectionTitle>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 2 }}>
          {(nodes?.positions ?? []).map((position) => (
            <Choice
              key={position}
              active={position === selection.hero}
              mono
              onClick={() => onChange({ ...selection, hero: position })}
            >
              {position}
            </Choice>
          ))}
        </div>
      </section>

      <section style={{ marginBottom: 18 }}>
        <SectionTitle>情境</SectionTitle>
        {groups.map(([group, items]) => (
          <div key={group} style={{ marginBottom: 8 }}>
            <div className="dim" style={{ fontSize: 10, margin: '0 0 3px' }}>
              {group}
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              {items.map((item) => (
                <Choice
                  key={item.key}
                  active={item.key === selection.scenario}
                  onClick={() => onChange({ ...selection, scenario: item.key })}
                >
                  {item.label}
                </Choice>
              ))}
            </div>
          </div>
        ))}
      </section>

      <section style={{ marginBottom: 18 }}>
        <SectionTitle>有效籌碼</SectionTitle>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {(nodes?.buckets ?? []).map((bucket) => (
            <Choice
              key={bucket.key}
              active={bucket.key === selection.bucket}
              onClick={() => onChange({ ...selection, bucket: bucket.key })}
            >
              <span style={{ fontFamily: 'var(--font-mono)' }}>{bucket.label}</span>
              <span className="dim" style={{ fontSize: 10, marginLeft: 'auto' }}>
                {bucket.pushFold ? '推入或棄牌' : `×${(bucket.multiplier / 10000).toFixed(2)}`}
              </span>
            </Choice>
          ))}
        </div>
        <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
          規則細則 8.5 的 9 檔。乘數是該檔對範圍寬度的縮放。
        </div>
      </section>

      {overrideCount > 0 && (
        <div
          style={{
            padding: '6px 8px',
            border: '1px solid var(--warning)',
            borderRadius: 'var(--radius-control)',
            color: 'var(--warning)',
            fontSize: 11,
          }}
        >
          自身策略已覆寫 {overrideCount} 格
        </div>
      )}
    </>
  );
}

function groupScenarios(
  nodes: StrategyNodesView | null,
): Array<[string, StrategyNodesView['scenarios']]> {
  const out: Array<[string, StrategyNodesView['scenarios']]> = [];
  for (const item of nodes?.scenarios ?? []) {
    const last = out[out.length - 1];
    if (last && last[0] === item.group) last[1].push(item);
    else out.push([item.group, [item]]);
  }
  return out;
}

/**
 * 導航的一列。
 *
 * 選中態依 V.4 為整格直角背景色塊填滿，不用左側強調邊框或側邊指示條。
 */
function Choice({
  active,
  mono,
  onClick,
  children,
}: {
  active: boolean;
  mono?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        width: '100%',
        padding: '5px 8px',
        border: '1px solid transparent',
        borderRadius: 0,
        background: active ? 'var(--bg-hover)' : 'transparent',
        color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
        fontWeight: active ? 600 : 400,
        fontSize: 12,
        fontFamily: mono ? 'var(--font-mono)' : 'inherit',
        textAlign: 'left',
        cursor: 'pointer',
      }}
    >
      {children}
    </button>
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
