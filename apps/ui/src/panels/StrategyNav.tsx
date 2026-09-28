// 面板 D — 情境導航（左欄）。
//
// UI 規格 D.1 的版面：左側情境導航樹（桌型 → 位置 → 情境 → 籌碼分檔），
// 右側編輯區。這一欄只決定「看哪個節點」，範圍矩陣本身在右側資訊窗。
//
// 清單一律由引擎列舉（`strategyNodes`），前端不自己算哪些情境到得了。
// 自己算的話遲早會列出 UTG「面對開牌」這種到不了的節點，使用者會在
// 一個永遠不會被查到的格子上編輯。

import { useEffect, useState } from 'react';
import type {
  OpenTierBoundsView,
  PostflopStrategyView,
  StrategyNodesView,
} from '../../../../packages/poker-types/src/index';
import { postflopStrategy, strategyNodes } from '../api';
import { Segmented } from '../components/Field';

export type StrategyStage = 'preflop' | 'flop' | 'turn' | 'river';

/** 目前檢視中的策略節點。翻前與翻後共用同一份面板選取狀態。 */
export interface StrategySelection {
  stage: StrategyStage;
  situation: string;
  seated: number;
  hero: string;
  bucket: string;
  scenario: string;
}

export const DEFAULT_SELECTION: StrategySelection = {
  stage: 'preflop',
  situation: 'no-bet',
  seated: 9,
  hero: 'BTN',
  bucket: '160-240',
  scenario: 'unopened',
};

/**
 * 情境鍵拆成基礎情境與 open 尺度區間後綴。
 *
 * 面對開牌的中型／大型區間是標準節點的切片，鍵為 `vs-open-UTG@open-large`。
 * 導航清單只列基礎情境，區間另外選。
 */
export function splitScenario(scenario: string): { base: string; suffix: string } {
  const at = scenario.indexOf('@');
  return at < 0
    ? { base: scenario, suffix: '' }
    : { base: scenario.slice(0, at), suffix: scenario.slice(at) };
}

/** 面對單一開牌（不含開牌＋再加注）才有 open 尺度區間 */
export function isVsOpen(scenario: string): boolean {
  return scenario.startsWith('vs-open-') && !scenario.startsWith('vs-open-raise-');
}

/** 區間的 BB 範圍說明，邊界由使用者設定 */
export function openTierRange(key: string, bounds: OpenTierBoundsView): string {
  const bb = (centi: number) => `${Number((centi / 100).toFixed(2))} BB`;
  switch (key) {
    case 'standard':
      return `≤ ${bb(bounds.mediumAboveCentiBb)}`;
    case 'medium':
      return `> ${bb(bounds.mediumAboveCentiBb)}～${bb(bounds.largeAboveCentiBb)}`;
    default:
      return `> ${bb(bounds.largeAboveCentiBb)}`;
  }
}

export function StrategyNav({
  selection,
  onChange,
  overrideCount,
  openTiers,
}: {
  selection: StrategySelection;
  onChange: (selection: StrategySelection) => void;
  /** 全部節點的覆寫筆數，供左欄一眼看出自己改了多少 */
  overrideCount: number;
  /** open 尺度區間邊界，只用來顯示各區間的 BB 範圍 */
  openTiers: OpenTierBoundsView;
}) {
  const [nodes, setNodes] = useState<StrategyNodesView | null>(null);
  const [postflop, setPostflop] = useState<PostflopStrategyView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    postflopStrategy()
      .then((view) => {
        if (!disposed) setPostflop(view);
      })
      .catch((error: unknown) => {
        if (!disposed) setFailure(String(error));
      });
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    if (selection.stage !== 'preflop') return undefined;
    let disposed = false;
    strategyNodes(selection.seated, selection.hero)
      .then((view) => {
        if (disposed) return;
        setNodes(view);
        setFailure(null);
        // 桌型換小之後，原本的位置或情境可能不存在（9 人桌的 UTG+2 在
        // 6 人桌沒有）。引擎回的是修正後的結果，這裡同步回選取狀態，
        // 否則左欄顯示的節點與右側矩陣畫的會是兩件事
        const { base } = splitScenario(selection.scenario);
        const scenarioOk = view.scenarios.some((item) => item.key === base);
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
  const { base: baseScenario, suffix: tierSuffix } = splitScenario(selection.scenario);
  const stages: Array<{ key: StrategyStage; label: string }> = [
    { key: 'preflop', label: '翻前' },
    ...((postflop?.streets ?? [
      { key: 'flop', label: '翻牌' },
      { key: 'turn', label: '轉牌' },
      { key: 'river', label: '河牌' },
    ]) as Array<{ key: StrategyStage; label: string }>),
  ];

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
        <SectionTitle>階段</SectionTitle>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 2 }}>
          {stages.map((stage) => (
            <Choice
              key={stage.key}
              active={selection.stage === stage.key}
              onClick={() => onChange({ ...selection, stage: stage.key })}
            >
              {stage.label}
            </Choice>
          ))}
        </div>
      </section>

      {selection.stage === 'preflop' && (
        <>
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
                  active={item.key === baseScenario}
                  // 換開牌者時保留目前的 open 區間；換到其他情境則沒有區間
                  onClick={() =>
                    onChange({
                      ...selection,
                      scenario: isVsOpen(item.key) ? item.key + tierSuffix : item.key,
                    })
                  }
                >
                  {item.label}
                </Choice>
              ))}
            </div>
          </div>
        ))}
      </section>

      {isVsOpen(baseScenario) && (
        <section style={{ marginBottom: 18 }}>
          <SectionTitle>對手 OPEN 尺度</SectionTitle>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {(nodes?.openTiers ?? []).map((tier) => (
              <Choice
                key={tier.key}
                active={tier.suffix === tierSuffix}
                onClick={() => onChange({ ...selection, scenario: baseScenario + tier.suffix })}
              >
                {tier.label}
                <span className="dim" style={{ fontSize: 10, marginLeft: 'auto', fontFamily: 'var(--font-mono)' }}>
                  {openTierRange(tier.key, openTiers)}
                </span>
              </Choice>
            ))}
          </div>
          <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
            依對手實際 open 的 raise-to 總額分區間。中型／大型未設定的格子沿用標準區間；
            區間邊界在右側「OPEN 尺度區間」調整。
          </div>
        </section>
      )}

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
              {/* 引擎分九檔、顧問的表只有四檔。選了 40–70BB 實際看到的是
                  表上 35–50BB 那一欄，不寫出來的話畫面上毫無跡象 */}
              <span className="dim" style={{ fontSize: 10, marginLeft: 'auto' }}>
                → {bucket.chartDepth}
              </span>
            </Choice>
          ))}
        </div>
        <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
          規則細則 8.5 的 9 檔，右側為對應到預設組合表的深度欄。
        </div>
      </section>

        </>
      )}

      {selection.stage !== 'preflop' && (
        <>
          <section style={{ marginBottom: 18 }}>
            <SectionTitle>下注狀態</SectionTitle>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              {(postflop?.situations ?? []).map((situation) => (
                <Choice
                  key={situation.key}
                  active={selection.situation === situation.key}
                  onClick={() => onChange({ ...selection, situation: situation.key })}
                >
                  {situation.label}
                </Choice>
              ))}
            </div>
          </section>

          <section style={{ marginBottom: 18 }}>
            <SectionTitle>牌面分類</SectionTitle>
            {[
              { dimension: '牌面外觀', options: postflop?.surfaces ?? [] },
              { dimension: '順子結構', options: postflop?.connectivities ?? [] },
            ].map(({ dimension, options }) => (
              <div key={dimension} style={{ marginBottom: 10 }}>
                <div className="dim" style={{ fontSize: 10, marginBottom: 3 }}>
                  {dimension}
                </div>
                {options.map((texture) => (
                  <div
                    key={texture.key}
                    title={texture.description}
                    style={{ padding: '3px 8px', fontSize: 11, color: 'var(--text-secondary)' }}
                  >
                    {texture.label}
                  </div>
                ))}
              </div>
            ))}
            <div className="dim" style={{ fontSize: 10, lineHeight: 1.5 }}>
              每個牌面同時命中一個花色／公對類型與一個乾／濕類型。
            </div>
          </section>
        </>
      )}

      {selection.stage === 'preflop' && overrideCount > 0 && (
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
  // 依分組合併，保留各組第一次出現的順序。引擎的情境清單是依開牌者交錯
  // 排列（面對 UTG 開牌、面對 UTG 開牌＋再加注、面對 UTG+1 開牌…），只合併
  // 相鄰項目會產生同名分組，React key 重複後切換位置時會殘留舊項目
  const out = new Map<string, StrategyNodesView['scenarios']>();
  for (const item of nodes?.scenarios ?? []) {
    const group = out.get(item.group);
    if (group) group.push(item);
    else out.set(item.group, [item]);
  }
  return [...out.entries()];
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
