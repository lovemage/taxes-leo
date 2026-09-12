// 面板 D — 自身策略（右側資訊窗）。
//
// UI 規格 D.1：資訊密度最高的面板，編輯「自身座位」的策略。節點在左欄
// 選，這裡畫該節點的 169 格範圍矩陣並提供逐格編輯。
//
// # 翻前矩陣與翻後節點
//
// 翻前走參數化 baseline，位置、籌碼分檔與情境真的會改變分佈，攤開來
// 是有內容的。翻後依街別、下注狀態與八種牌面標籤呈現合法動作；頻率
// 仍由未簽核的 equity／牌面工程基準產生，因此畫面必須明確揭露內容狀態。
//
// # 頻率一律由引擎算
//
// 這個檔案不重算任何頻率、寬度或加注尺度。使用者改一格，前端做的只是
// 把「這一格改成多少」送回引擎，再把引擎重算的矩陣畫出來。UI 自己算的
// 話，面板顯示的範圍會與 Bot 實際打的漂移，而且完全沒有徵兆。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  CellOverrideView,
  ChartRowView,
  MatrixCellView,
  PostflopDiagnosticsView,
  PostflopHandPreviewView,
  PostflopNodesView,
  PostflopOverridesView,
  PostflopRuleQuery,
  PostflopRuleView,
  PostflopStaticCoverageView,
  PostflopStrategyView,
  RangeMatrixView,
  StrategyMetaView,
} from '../../../../packages/poker-types/src/index';
import {
  classifyPostflopHand,
  postflopDiagnostics,
  postflopNodes,
  postflopRule,
  postflopStrategy,
  strategyMatrix,
  strategyMeta,
} from '../api';
import { cellTone, FULL } from '../components/matrixTone';
import type { StrategySelection } from './StrategyNav';


export function Strategy({
  selection,
  overrides,
  onOverridesChange,
  postflopOverrides,
  onPostflopOverridesChange,
  locked,
}: {
  selection: StrategySelection;
  overrides: CellOverrideView[];
  onOverridesChange: (overrides: CellOverrideView[]) => void;
  postflopOverrides: PostflopOverridesView;
  onPostflopOverridesChange: (next: PostflopOverridesView) => void;
  /** run 進行中不得修改策略：內容是 RunManifest 快照的一部分 */
  locked: boolean;
}) {
  const [meta, setMeta] = useState<StrategyMetaView | null>(null);
  const [postflop, setPostflop] = useState<PostflopStrategyView | null>(null);
  const [matrix, setMatrix] = useState<RangeMatrixView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [pending, setPending] = useState(true);

  useEffect(() => {
    strategyMeta().then(setMeta).catch(() => setMeta(null));
    postflopStrategy().then(setPostflop).catch(() => setPostflop(null));
  }, []);

  // 回應可能亂序抵達（覆寫改動會連發好幾次請求）。只採用最後一次發出的
  // 請求，否則放開滑桿後畫面會跳回舊的矩陣
  const issued = useRef(0);

  useEffect(() => {
    if (selection.stage !== 'preflop') {
      setPending(false);
      return;
    }
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
  const streetLabel =
    selection.stage === 'preflop'
      ? null
      : postflop?.streets.find((street) => street.key === selection.stage)?.label;
  const situationLabel = postflop?.situations.find(
    (situation) => situation.key === selection.situation,
  )?.label;

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
            {selection.stage === 'preflop'
              ? matrix
                ? `${matrix.seated}-max ${matrix.hero}｜${matrix.scenarioLabel}`
                : '載入中'
              : `${streetLabel ?? '翻後'}｜${situationLabel ?? '載入中'}`}
          </span>
        </h2>
        <span className="dim" style={{ fontSize: 11, fontFamily: 'var(--font-mono)' }}>
          {selection.stage === 'preflop' ? matrix?.nodeKey ?? '' : ''}
        </span>
      </div>
      <p className="dim" style={{ fontSize: 11, margin: '0 0 16px', lineHeight: 1.6 }}>
        {selection.stage === 'preflop'
          ? '節點在左欄選。點任一格可改該手牌的行動頻率；改過的格會蓋掉內容產生的結果，並隨 run 寫進 RunManifest 快照。'
          : '左欄選擇街別與下注狀態。牌面同時套用花色／公對與乾／濕兩個維度；不成立的動作仍保留在表內並明確標示。'}
      </p>

      {failure && <Banner tone="negative">{failure}</Banner>}
      {locked && <Banner tone="warning">run 進行中，策略鎖定。</Banner>}
      {selection.stage === 'preflop' && meta && !meta.chartLoaded && (
        <Banner tone="negative">
          預設組合表<strong>沒有載進來</strong>：{meta.chartNote}
        </Banner>
      )}
      {selection.stage === 'preflop' && meta && meta.chartLoaded && matrix?.source === 'baseline' && (
        <Banner tone="warning">
          這個節點<strong>不在預設組合表上</strong>（表沒有「面對跛入」那一欄），
          畫的是 {meta.baselineName}（{meta.baselineVersion}）——參數化產生的工程佔位內容，
          未經牌手顧問簽核。
        </Banner>
      )}
      {/* 低樣本排序畫出來的矩陣與正式的長得一模一樣，使用者沒有任何辦法
          自己分辨。因此非內容級一律明講，而不是只在某個角落寫個取樣數 */}
      {selection.stage === 'preflop' && meta && !meta.rankingContentGrade && (
        <Banner tone="negative">
          equity 排序<strong>不是正式內容</strong>：{meta.rankingNote}
          。這個面板畫的範圍與此時跑出來的統計都只能當開發參考。
        </Banner>
      )}

      {selection.stage === 'preflop' && !matrix && !failure && (
        <section style={{ ...cardStyle, maxWidth: 420 }}>
          <SectionTitle>載入中</SectionTitle>
          <div className="dim" style={{ fontSize: 11, lineHeight: 1.6 }}>
            正在向引擎取這個節點的 169 格頻率。equity 排序是離線產製的資產，
            載入不需要等待，切換節點是即時的。
          </div>
        </section>
      )}

      {selection.stage === 'preflop' && matrix && (
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
              <Stat
                label="內容來源"
                value={matrix.source === 'chart' ? '預設組合表' : '參數 baseline'}
              />
              {/* 引擎分九檔籌碼、表只有四檔。使用者選了 40–70BB 看到的其實是
                  表上 35–50BB 那一欄，不寫出來的話畫面上毫無跡象 */}
              <Stat label="表的深度欄" value={matrix.chartDepth ?? '—'} />
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
              <Swatch color="var(--matrix-check)">過牌</Swatch>
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
            {matrix.chartRows.length > 0 && (
              <ChartCard
                rows={matrix.chartRows}
                scenario={matrix.chartScenario}
                depth={matrix.chartDepth}
                picked={cell?.chartAction ?? null}
              />
            )}
            {meta && <ContentCard meta={meta} />}
          </aside>
        </div>
      )}

      {selection.stage !== 'preflop' && (
        <PostflopRules
          stage={selection.stage}
          situationKey={selection.situation}
          overrides={postflopOverrides}
          onOverridesChange={onPostflopOverridesChange}
          locked={locked}
        />
      )}
    </div>
  );
}

function PostflopRules({
  stage,
  situationKey,
  overrides,
  onOverridesChange,
  locked,
}: {
  stage: string;
  situationKey: string;
  overrides: PostflopOverridesView;
  onOverridesChange: (next: PostflopOverridesView) => void;
  locked: boolean;
}) {
  const [nodes, setNodes] = useState<PostflopNodesView | null>(null);
  const [rule, setRule] = useState<PostflopRuleView | null>(null);
  const [line, setLine] = useState('cbet-chance');
  const [heroPosition, setHeroPosition] = useState('*');
  const [relativePosition, setRelativePosition] = useState('*');
  const [decisionPhase, setDecisionPhase] = useState('*');
  const [continuation, setContinuation] = useState('*');
  useEffect(() => { setDecisionPhase('*'); setContinuation('*'); }, [stage, situationKey, line]);
  const [surface, setSurface] = useState('rainbow');
  const [connectivity, setConnectivity] = useState('dry');
  const [handStrength, setHandStrength] = useState('strong-made');
  const [facingSize, setFacingSize] = useState('none');
  /** 未儲存的草稿。null 代表畫的就是引擎回傳的值 */
  const [draft, setDraft] = useState<Record<string, number> | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  /** 覆寫清單的歷史。恢復與一般編輯共用同一個 stack（計劃 §5.2） */
  const [undoStack, setUndoStack] = useState<PostflopOverridesView[]>([]);
  /** 批次恢復的二次確認。null 代表沒有待確認的操作 */
  const [pendingBatch, setPendingBatch] = useState<BatchScope | null>(null);
  const [diagnostics, setDiagnostics] = useState<PostflopDiagnosticsView | null>(null);

  useEffect(() => {
    postflopNodes(stage, overrides)
      .then((view) => {
        setNodes(view);
        setFailure(null);
      })
      .catch((error: unknown) => setFailure(String(error)));
  }, [stage, overrides]);

  // 下注狀態換了之後，線路與面對尺度必須跟著換到合法值——
  // 「面對 c-bet」不存在於無人下注，留著會查出一個不可達的節點
  useEffect(() => {
    if (!nodes) return;
    const allowed = nodes.lines.filter((item) => item.situation === situationKey);
    if (!allowed.some((item) => item.key === line)) {
      setLine(allowed[0]?.key ?? 'cbet-chance');
    }
    if (situationKey === 'no-bet') {
      setFacingSize('none');
    } else if (facingSize === 'none') {
      setFacingSize('half-to-two-thirds');
    }
  }, [nodes, situationKey, line, facingSize]);

  // 診斷要看的是「按下儲存之後的那一份清單」，不是上一次套用的那一份。
  // 只查已套用的覆寫，草稿裡的錯誤要等它存進去才會被指出來——
  // 而那個時候它已經生效了
  const candidate = useMemo<PostflopOverridesView>(() => {
    if (!draft || !rule) return overrides;
    const weights = rule.weights
      .filter((weight) => weight.available)
      .map((weight) => ({ kind: weight.kind, myriad: draft[weight.kind] ?? 0 }));
    return {
      ...overrides,
      nodes: [
        ...overrides.nodes.filter((item) => item.nodeKey !== rule.nodeKey),
        { nodeKey: rule.nodeKey, weights },
      ],
    };
  }, [overrides, draft, rule]);
  const candidateKey = JSON.stringify(candidate);

  useEffect(() => {
    postflopDiagnostics(JSON.parse(candidateKey) as PostflopOverridesView)
      .then(setDiagnostics)
      .catch(() => setDiagnostics(null));
  }, [candidateKey]);

  // 牌力組清單依街別過濾：河牌只有五組
  useEffect(() => {
    if (!nodes) return;
    if (!nodes.handStrengths.some((item) => item.key === handStrength)) {
      setHandStrength(nodes.handStrengths[0]?.key ?? 'strong-made');
    }
  }, [nodes, handStrength]);

  const query: PostflopRuleQuery = {
    scope: [heroPosition, relativePosition, decisionPhase, continuation].join('/'),
    street: stage,
    situation: situationKey,
    line,
    surface,
    connectivity,
    handStrength,
    facingSize,
  };
  const queryKey = JSON.stringify(query);

  // 回應可能亂序抵達。只採用最後一次發出的請求
  const issued = useRef(0);
  useEffect(() => {
    const ticket = ++issued.current;
    setRule(null); setDraft(null);
    postflopRule(JSON.parse(queryKey) as PostflopRuleQuery, overrides)
      .then((view) => {
        if (ticket !== issued.current) return;
        setRule(view);
        setDraft(null);
        setFailure(null);
      })
      .catch((error: unknown) => {
        if (ticket !== issued.current) return;
        setRule(null);
        setFailure(String(error));
      });
  }, [queryKey, overrides]);

  if (!nodes) {
    return (
      <section style={{ ...cardStyle, maxWidth: 420 }}>
        <SectionTitle>載入中</SectionTitle>
        <div className="dim" style={{ fontSize: 11 }}>正在取得翻後策略節點。</div>
      </section>
    );
  }

  const lines = nodes.lines.filter((item) => item.situation === situationKey);
  const facingSizes = nodes.facingSizes.filter((item) =>
    situationKey === 'no-bet' ? item.key === 'none' : item.key !== 'none',
  );
  const nodeKey = rule?.nodeKey ?? '';
  const current: Record<string, number> =
    draft ?? Object.fromEntries((rule?.weights ?? []).map((weight) => [weight.kind, weight.myriad]));
  const total = (rule?.weights ?? [])
    .filter((weight) => weight.available)
    .reduce((sum, weight) => sum + (current[weight.kind] ?? 0), 0);
  const dirty = draft !== null;
  // 有 error 就不得保存（UI 規格 D.8）。warning 可保存，留在驗證摘要
  const blocked = (diagnostics?.errorCount ?? 0) > 0;
  const canSave = !locked && dirty && total === FULL && !blocked;

  const setWeight = (kind: string, myriad: number) =>
    setDraft({ ...current, [kind]: Math.max(0, Math.min(FULL, myriad)) });

  /** 每一次改動都先把現況推進歷史，恢復與一般編輯因此共用同一個 undo */
  const apply = (next: PostflopOverridesView) => {
    setUndoStack((stack) => [...stack.slice(-19), overrides]);
    setPendingBatch(null);
    onOverridesChange(next);
  };

  const undo = () => {
    const previous = undoStack.at(-1);
    if (!previous) return;
    setUndoStack((stack) => stack.slice(0, -1));
    setPendingBatch(null);
    onOverridesChange(previous);
  };

  // 存的就是診斷剛剛驗過的那一份，兩邊各組一次的話會漂移
  const save = () => apply(candidate);

  const restore = () =>
    apply({
      ...overrides,
      nodes: overrides.nodes.filter((item) => item.nodeKey !== nodeKey),
    });

  /** 批次恢復的範圍判定。節點鍵的欄位順序見引擎的 `PostflopNode::key` */
  const inScope = (scope: BatchScope, key: string): boolean => {
    const parts = key.split('|');
    if (parts.length !== 7 && parts.length !== 8) return false;
    switch (scope) {
      case 'street':
        return parts[0] === stage;
      case 'line':
        return parts[0] === stage && parts[2] === line;
      case 'hand-strength':
        return parts[5] === handStrength;
      default:
        return false;
    }
  };

  const batchCount = (scope: BatchScope) =>
    overrides.nodes.filter((item) => inScope(scope, item.nodeKey)).length;

  const batchRestore = (scope: BatchScope) => {
    if (pendingBatch !== scope) {
      // 第一次點只是要求確認：批次恢復可能一次清掉數十筆
      setPendingBatch(scope);
      return;
    }
    apply({
      ...overrides,
      nodes: overrides.nodes.filter((item) => !inScope(scope, item.nodeKey)),
    });
  };

  return (
    <div style={{ display: 'grid', gap: 14 }}>
      {failure && <Banner tone="negative">{failure}</Banner>}
      {!nodes.consultantApproved && <Banner tone="warning">{nodes.note}</Banner>}

      {/* ── 導覽：線路 → 牌面兩軸 → 牌力組 → 面對尺度 ── */}
      <section style={{ ...cardStyle, display: 'grid', gap: 12 }}>
        <OptionRow label="翻前位置" value={heroPosition} onChange={setHeroPosition}
          options={['*','UTG','UTG+1','UTG+2','LJ','HJ','CO','BTN','SB','BB'].map(key => ({key,label:key==='*'?'所有位置':key}))} />
        <OptionRow label="翻後相對位置" value={relativePosition} onChange={setRelativePosition}
          options={[{key:'*',label:'不限'},{key:'first',label:'最先行動（OOP）'},{key:'middle',label:'中間（多人底池）'},{key:'last',label:'最後行動（IP）'},{key:'alone',label:'其餘對手皆全下'}]} />
        <OptionRow label="本街行動" value={decisionPhase} onChange={setDecisionPhase}
          options={[{key:'*',label:'不限'},{key:'unacted',label:'尚未行動'},...(situationKey==='facing-bet'?[{key:'checked',label:'過牌後被下注（可 check-raise）'},{key:'bet-raised',label:'已投入後面對加注'}]:[])]} />
        <OptionRow label="持續下注歷史" value={continuation} onChange={setContinuation}
          options={[{key:'*',label:'不限'},...(stage==='flop'?[{key:'flop-cbet',label:'翻牌 C-bet'}]:stage==='turn'?[{key:'double-barrel',label:'第二槍 Double barrel'},{key:'delayed-cbet',label:'延遲 C-bet'}]:[{key:'triple-barrel',label:'第三槍 Triple barrel'},{key:'delayed-cbet',label:'延遲 C-bet'}]),{key:'other',label:'其他（含加注後延續下注）'}]} />
        <p className="dim" style={{margin:0,fontSize:12}}>位置依仍可行動的對手判斷。完整度只計通用節點，不含位置／歷史限定。先在無人下注設定過牌頻率，再於「過牌後被下注」設定跟注／加注頻率；加注仍須符合實際加注權。</p>
        <OptionRow
          label="牌局線路"
          options={lines.map((item) => ({ key: item.key, label: item.label }))}
          value={line}
          onChange={setLine}
        />
        <OptionRow
          label="牌面外觀"
          options={nodes.surfaces.map((item) => ({
            key: item.key,
            label: item.label,
            title: item.description,
          }))}
          value={surface}
          onChange={setSurface}
        />
        <OptionRow
          label="順子結構"
          options={nodes.connectivities.map((item) => ({
            key: item.key,
            label: item.label,
            title: item.description,
          }))}
          value={connectivity}
          onChange={setConnectivity}
        />
        <OptionRow
          label="牌力組"
          options={nodes.handStrengths.map((item) => ({
            key: item.key,
            label: item.label,
            title: item.description,
          }))}
          value={handStrength}
          onChange={setHandStrength}
        />
        {situationKey === 'facing-bet' && (
          <OptionRow
            label="面對下注區間"
            options={facingSizes.map((item) => ({ key: item.key, label: item.label }))}
            value={facingSize}
            onChange={setFacingSize}
          />
        )}
      </section>

      {/* ── 頻率編輯器 ── */}
      {rule && (
        <section style={{ ...cardStyle, display: 'grid', gap: 12, maxWidth: 720 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
            <div>
              <SectionTitle>行動頻率</SectionTitle>
              <div className="dim" style={{ fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                {rule.nodeKey}
              </div>
            </div>
            <div style={{ textAlign: 'right', fontSize: 11 }}>
              <div>
                來源：<strong>{rule.sourceLabel}</strong>
                {rule.ruleId && (
                  <span className="dim" style={{ marginLeft: 6 }}>{rule.ruleId}</span>
                )}
              </div>
              <div className="dim">
                {rule.consultantApproved ? '已由顧問簽核' : '未簽核內容'}
              </div>
            </div>
          </div>

          <div style={{ display: 'grid', gap: 6 }}>
            {rule.weights.map((weight) => (
              <div
                key={weight.kind}
                style={{
                  display: 'grid',
                  gridTemplateColumns: '150px 90px 1fr',
                  alignItems: 'center',
                  gap: 10,
                  opacity: weight.available ? 1 : 0.45,
                }}
              >
                <span style={{ fontSize: 12 }}>{weight.label}</span>
                <input
                  type="number"
                  aria-label={`${weight.label} 頻率`}
                  min={0}
                  max={100}
                  step={1}
                  disabled={!weight.available || locked}
                  value={Math.round((current[weight.kind] ?? 0) / 100)}
                  onChange={(event) =>
                    setWeight(weight.kind, Math.round(Number(event.target.value) * 100))
                  }
                  style={{
                    width: 72,
                    padding: '4px 6px',
                    fontSize: 12,
                    fontFamily: 'var(--font-mono)',
                    border: '1px solid var(--border)',
                    borderRadius: 'var(--radius-control)',
                    background: 'var(--bg-surface)',
                    color: 'inherit',
                  }}
                />
                <span className="dim" style={{ fontSize: 10 }}>
                  {weight.available ? '%' : `不成立——${weight.unavailableReason ?? ''}`}
                </span>
              </div>
            ))}
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
            <span style={{ fontSize: 12 }}>
              合計{' '}
              <strong style={{ color: total === FULL ? 'inherit' : 'var(--negative)' }}>
                {pct(total)}
              </strong>
            </span>
            {total !== FULL && (
              <span className="dim" style={{ fontSize: 11 }}>
                合計必須是 100% 才能儲存
              </span>
            )}
            {blocked && (
              <span style={{ fontSize: 11, color: 'var(--negative)' }}>
                存下去會有 {diagnostics?.errorCount} 個必須修正的問題，先處理才能儲存
              </span>
            )}
            <Chip disabled={!canSave} onClick={save}>
              儲存這個節點
            </Chip>
            <Chip disabled={!dirty} onClick={() => setDraft(null)}>
              放棄修改
            </Chip>
            <Chip disabled={!rule.restore.enabled || locked} onClick={restore}>
              恢復繼承值
            </Chip>
          </div>

          <div className="dim" style={{ fontSize: 11, lineHeight: 1.6 }}>
            {rule.restore.reason}
          </div>

          {/* 批次恢復：執行前顯示受影響節點數，並要求二次確認 */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            <span className="dim" style={{ fontSize: 11 }}>
              批次恢復
            </span>
            {(
              [
                ['line', '這條線路'],
                ['street', '整街'],
                ['hand-strength', '這個牌力組'],
              ] as [BatchScope, string][]
            ).map(([scope, label]) => {
              const count = batchCount(scope);
              const confirming = pendingBatch === scope;
              return (
                <Chip
                  key={scope}
                  disabled={locked || count === 0}
                  onClick={() => batchRestore(scope)}
                >
                  {confirming ? `再按一次清除 ${count} 筆` : `${label}（${count}）`}
                </Chip>
              );
            })}
            <Chip disabled={undoStack.length === 0} onClick={undo}>
              復原（{undoStack.length}）
            </Chip>
          </div>

          {/* 計劃 §6.1：覆寫目前只活在這次開啟的視窗裡。不做本機持久化
              的話必須明白告知，不能留白讓使用者以為存下來了 */}
          <div className="dim" style={{ fontSize: 10, lineHeight: 1.6 }}>
            覆寫目前只保存在這次開啟的視窗，<strong>關掉重開會清空</strong>；
            策略庫（命名存檔、複製、匯出）是另一個功能，尚未實作。
            進行中的 run 已在開始時凍結快照，這裡的修改不影響它。
          </div>
        </section>
      )}

      {/* ── 驗證摘要（UI 規格 D.8）── */}
      {diagnostics && diagnostics.issues.length > 0 && (
        <section style={{ ...cardStyle, maxWidth: 720 }}>
          <SectionTitle>
            驗證摘要（{diagnostics.errorCount} 個錯誤、{diagnostics.warningCount} 個警告）
          </SectionTitle>
          <div className="dim" style={{ fontSize: 10, margin: '4px 0 8px', lineHeight: 1.6 }}>
            遮蔽、重疊與不可達是警告：幾乎一定是寫錯，但擋住保存會讓你連暫存都做不到。
            只有條件矛盾與來源排序錯誤會阻擋儲存。
          </div>
          <div style={{ display: 'grid', gap: 6 }}>
            {diagnostics.issues.slice(0, 12).map((issue, index) => (
              <div
                key={`${issue.ruleId}-${issue.kind}-${index}`}
                style={{
                  display: 'grid',
                  gridTemplateColumns: '64px 1fr',
                  gap: 10,
                  fontSize: 11,
                  lineHeight: 1.6,
                }}
              >
                <span
                  style={{
                    color: issue.severity === 'error' ? 'var(--negative)' : 'var(--warning)',
                    fontWeight: 600,
                  }}
                >
                  {issue.severity === 'error' ? '錯誤' : '警告'}
                </span>
                <span>
                  <strong>{issue.ruleName}</strong>
                  <span className="dim" style={{ marginLeft: 6 }}>{issue.message}</span>
                </span>
              </div>
            ))}
            {diagnostics.issues.length > 12 && (
              <div className="dim" style={{ fontSize: 10 }}>
                還有 {diagnostics.issues.length - 12} 項，依來源層級排序後只列前 12 項。
              </div>
            )}
          </div>
        </section>
      )}

      {/* ── 完整度與分類預覽 ── */}
      <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', alignItems: 'flex-start' }}>
        <CoverageCard coverage={nodes.coverage} />
        <HandPreviewCard />
      </div>
    </div>
  );
}

/** 批次恢復的範圍。整條線路／整街／整個牌力組（計劃 §5.2）。 */
type BatchScope = 'line' | 'street' | 'hand-strength';

/** 一列可選項。作用中用整格背景填滿＋文字加深，不用左側強調邊框。 */
function OptionRow({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { key: string; label: string; title?: string }[];
  value: string;
  onChange: (key: string) => void;
}) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '80px 1fr', gap: 10, alignItems: 'start' }}>
      <span className="dim" style={{ fontSize: 11, paddingTop: 4 }}>
        {label}
      </span>
      <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
        {options.map((option) => {
          const active = option.key === value;
          return (
            <button
              key={option.key}
              type="button"
              title={option.title}
              aria-pressed={active}
              onClick={() => onChange(option.key)}
              style={{
                padding: '4px 10px',
                border: '1px solid var(--border)',
                borderRadius: 0,
                background: active ? 'var(--bg-raised)' : 'transparent',
                color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                fontWeight: active ? 600 : 400,
                fontSize: 11,
                fontFamily: 'inherit',
                cursor: 'pointer',
              }}
            >
              {option.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** 靜態節點覆蓋。與報表的執行期覆蓋是兩個不同的指標，因此標題講清楚。 */
function CoverageCard({ coverage }: { coverage: PostflopStaticCoverageView }) {
  const rows: [string, number][] = [
    ['你的覆寫', coverage.user],
    ['官方內容', coverage.official],
    ['同組通則', coverage.generic],
    ['工程 fallback', coverage.fallback],
  ];
  return (
    <section style={{ ...cardStyle, flex: '1 1 280px', maxWidth: 360 }}>
      <SectionTitle>節點覆蓋</SectionTitle>
      <div className="dim" style={{ fontSize: 10, marginBottom: 8 }}>
        分母是可達節點數（{coverage.totalNodes}），與報表的「執行期決策次數」
        是兩個不同的指標。節點集合版本 {coverage.nodeSetVersion}
      </div>
      {rows.map(([label, count]) => (
        <Row
          key={label}
          label={label}
          value={`${count}（${pct(
            coverage.totalNodes === 0
              ? 0
              : Math.round((count * FULL) / Number(coverage.totalNodes)),
          )}）`}
        />
      ))}
      <Row label="你的完整度" value={pct(coverage.completenessMyriad)} />
    </section>
  );
}

/** 指定底牌的分類預覽。節點編輯器只顯示牌力組，單手資料放這裡。 */
function HandPreviewCard() {
  const [hole, setHole] = useState('As Ts');
  const [board, setBoard] = useState('Ah 7s 3s');
  const [preview, setPreview] = useState<PostflopHandPreviewView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    classifyPostflopHand(hole, board)
      .then((view) => {
        setPreview(view);
        setError(null);
      })
      .catch((failure: unknown) => {
        setPreview(null);
        setError(String(failure));
      });
  }, [hole, board]);

  const field: React.CSSProperties = {
    padding: '4px 6px',
    fontSize: 12,
    fontFamily: 'var(--font-mono)',
    border: '1px solid var(--border)',
    borderRadius: 'var(--radius-control)',
    background: 'var(--bg-surface)',
    color: 'inherit',
    width: 120,
  };

  return (
    <section style={{ ...cardStyle, flex: '1 1 320px', maxWidth: 420 }}>
      <SectionTitle>指定底牌分類預覽</SectionTitle>
      <div style={{ display: 'flex', gap: 8, margin: '8px 0 10px', flexWrap: 'wrap' }}>
        <input style={field} value={hole} onChange={(event) => setHole(event.target.value)} />
        <input style={field} value={board} onChange={(event) => setBoard(event.target.value)} />
      </div>
      {error && (
        <div className="dim" style={{ fontSize: 11, color: 'var(--negative)' }}>
          {error}
        </div>
      )}
      {preview && (
        <>
          <Row label="牌力組" value={preview.handStrengthLabel} />
          <Row label="牌面" value={`${preview.surface}＋${preview.connectivity}`} />
          <Row label="百分位" value={pct(preview.percentileMyriad)} />
          <Row
            label="有效補牌"
            value={`${(preview.effectiveOutsCenti / 100).toFixed(2)} 張（其中聽牌 ${(
              preview.drawOutsCenti / 100
            ).toFixed(2)}）`}
          />
          <Row label="改善公共牌" value={preview.improvesBoard ? '是' : '否'} />
          <div className="dim" style={{ fontSize: 10, marginTop: 8, lineHeight: 1.6 }}>
            對手模型：{preview.opponentModel}（對單一對手的牌力序位，均勻隨機合法組合）。
            門檻{preview.consultantApproved ? '已' : '尚未'}經顧問簽核。
          </div>
        </>
      )}
    </section>
  );
}

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
      title={[
        cell.class,
        cell.chartAction ? `表：${chartActionLabel(cell.chartAction)}` : null,
        `主動 ${pct(cell.aggressive)}`,
        `跟注 ${pct(cell.call)}`,
        cell.check > 0 ? `過牌 ${pct(cell.check)}` : null,
        `棄牌 ${pct(cell.fold)}`,
      ]
        .filter(Boolean)
        .join('｜')}
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


/** 表上的動作鍵 → 中文。與引擎的 `ChartAction::label` 同一組字 */
function chartActionLabel(action: string): string {
  switch (action) {
    case 'fold':
      return '蓋牌';
    case 'call':
      return '跟注';
    case 'raise-2.5x':
      return '加注（前方 2.5 倍）';
    case 'raise-8x':
      return '加注（前方 8 倍）';
    case 'allin':
      return 'ALL IN';
    default:
      return action;
  }
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

      {cell.chartAction && (
        <div
          style={{
            fontSize: 11,
            padding: '4px 6px',
            marginBottom: 8,
            background: 'var(--matrix-empty)',
            color: 'var(--text-secondary)',
          }}
        >
          預設組合表：<strong>{chartActionLabel(cell.chartAction)}</strong>
        </div>
      )}

      <Freq label="主動" value={cell.aggressive} locked={locked} onChange={setAggressive} />
      <Freq label="跟注" value={cell.call} locked={locked} onChange={setCall} />
      {/* 過牌只顯示不編輯：覆寫的結構是「主動 ＋ 跟注，棄牌為餘數」，
          裝不下第四個動作。硬塞一支滑桿只會讓使用者以為改得動 */}
      {cell.check > 0 && (
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            fontSize: 11,
            padding: '4px 0',
            color: 'var(--text-secondary)',
          }}
        >
          <span>過牌（無人加注，不需跟注）</span>
          <span className="num">{pct(cell.check)}</span>
        </div>
      )}
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
          ? '這一格是你的覆寫，內容不再影響它。'
          : cell.chartAction
            ? '目前照預設組合表。改動後即成為覆寫，蓋掉表上的那一格。'
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


/**
 * 表上這一格的五列原文。
 *
 * 面板 D 原本只畫得出「主動／跟注／棄牌」三個彙總頻率，那會把顧問寫的
 * 兩種加注尺寸壓成同一件事。這張卡片照表逐列攤開，包含 H 欄的原文說明
 * ——那是內容的一部分，不是註解。
 */
function ChartCard({
  rows,
  scenario,
  depth,
  picked,
}: {
  rows: ChartRowView[];
  scenario: string | null;
  depth: string | null;
  picked: string | null;
}) {
  return (
    <section style={cardStyle}>
      <SectionTitle>預設組合表</SectionTitle>
      <div className="dim" style={{ fontSize: 10, marginBottom: 8, lineHeight: 1.5 }}>
        {[depth, scenario].filter(Boolean).join('｜')}
      </div>
      <div style={{ display: 'grid', gap: 8 }}>
        {rows.map((row) => (
          <div
            key={row.action}
            style={{
              padding: '6px 8px',
              background: row.action === picked ? 'var(--matrix-empty)' : 'transparent',
              borderTop: '1px solid var(--border)',
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'baseline',
                gap: 8,
                fontSize: 11,
              }}
            >
              <span style={{ color: 'var(--text-primary)' }}>{row.label}</span>
              <span className="num" style={{ color: 'var(--text-secondary)' }}>
                {row.combos === 0 ? '無' : `${pct(row.shareMyriad)}｜${row.combos} combo`}
              </span>
            </div>
            {row.raiseToCentiBb !== null && (
              <div className="dim" style={{ fontSize: 10, marginTop: 2 }}>
                加注到 {(row.raiseToCentiBb / 100).toFixed(2)} BB
              </div>
            )}
            {row.note && (
              <div
                className="dim"
                style={{ fontSize: 10, marginTop: 3, lineHeight: 1.5 }}
              >
                {row.note}
              </div>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}

/** D.6／D.7：內容來源、加注尺度與翻後 fallback，如實揭露 */
function ContentCard({ meta }: { meta: StrategyMetaView }) {
  return (
    <section style={cardStyle}>
      <SectionTitle>內容現況</SectionTitle>
      <Row label="預設組合表" value={meta.chartSource} />
      <Row label="表版本" value={meta.chartVersion} />
      <Row label="表格數" value={`${meta.chartCellCount.toLocaleString()} 格`} />
      <Row label="表涵蓋節點" value={`${pct(meta.chartCoverageMyriad)}`} />
      <Row label="參數基準" value={`${meta.baselineName}`} />
      <Row label="參數版本" value={meta.baselineVersion} />
      <Row label="參數簽核" value={meta.consultantApproved ? '已簽核' : '未簽核'} />
      <Row label="翻前節點" value={`${meta.preflopNodeCount.toLocaleString()} 個`} />
      <Row label="翻前格數" value={`${meta.preflopCellCount.toLocaleString()} 格`} />
      <Row label="開牌尺度" value={`${(meta.openSizeCentiBb / 100).toFixed(2)} BB`} />
      <Row label="3-bet 尺度" value={`${(meta.threeBetSizeCentiBb / 100).toFixed(2)} BB`} />
      <Row label="4-bet 尺度" value={`${(meta.fourBetSizeCentiBb / 100).toFixed(2)} BB`} />
      <Row label="推入門檻" value={`${meta.pushFoldBelow} BB 以下`} />
      <Row label="equity 取樣" value={meta.rankingSamples.toLocaleString()} />
      <Row label="排序來源" value={meta.rankingSource} />
      <Row
        label="排序等級"
        value={meta.rankingContentGrade ? '內容級' : '非正式內容'}
      />
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
        <div style={{ marginBottom: 4 }}>{meta.chartNote}</div>
        <div style={{ marginBottom: 4 }}>{meta.rankingNote}</div>
        <div style={{ marginBottom: 4 }}>
          翻後工程基準（
          <span style={{ fontFamily: 'var(--font-mono)' }}>{meta.postflopBaseline}</span>）。
        </div>
        <span className="dim">
          已依牌力、存活對手數與底池賠率產生下注／跟注／加注分佈；顧問規則表
          尚未進來，因此這版仍屬未簽核工程內容。異常時保底：{meta.postflopFallback}。
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
