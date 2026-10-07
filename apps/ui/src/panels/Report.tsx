import { useEffect, useState } from 'react';
import type { EstimateView, ProportionView, ReportView } from '../../../../packages/poker-types/src/index';
import { exportFile, getReport } from '../api';
import { reportCsv } from '../reportCsv';
import { PreflopHitsCard } from '../components/PreflopHitsCard';

const number = (value: number | null) => value === null || !Number.isFinite(value) ? '—' : value.toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const percent = (value: number | null) => value === null ? '—' : `${number(value * 100)}%`;
const interval = (low: number | null, high: number | null, percentage = false) =>
  low === null || high === null ? '樣本不足' : `${percentage ? percent(low) : number(low)} ～ ${percentage ? percent(high) : number(high)}`;

function Rate({ rate }: { rate: ProportionView }) {
  return <><strong className="num">{percent(rate.point)}</strong><small>{rate.numerator.toLocaleString()} / {rate.denominator.toLocaleString()} · 95% CI {interval(rate.ciLow, rate.ciHigh, true)}</small></>;
}

function Estimate({ ev }: { ev: EstimateView }) {
  return <><strong className="num">{number(ev.point)} bb/100</strong><small>95% CI {interval(ev.ciLow, ev.ciHigh)}</small><small>{ev.verdict}</small></>;
}

export function Report({ reloadToken, running }: { reloadToken: number; running: boolean }) {
  const [report, setReport] = useState<ReportView | null>(null);
  const [includeDead, setIncludeDead] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [exporting, setExporting] = useState(false);
  const [exportMessage, setExportMessage] = useState('');
  useEffect(() => {
    let active = true;
    setLoading(true); setError(''); setReport(null); setExportMessage('');
    getReport(includeDead).then(next => { if (active) setReport(next); })
      .catch(error => { if (active) setError(String(error)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [reloadToken, includeDead, refresh]);

  async function exportCsv() {
    if (!report) return;
    setExporting(true); setExportMessage('');
    try {
      const path = await exportFile(`9max-report-run-${report.scope.runId}${report.includeDead ? '-include-dead' : ''}.csv`, reportCsv(report), 'text/csv;charset=utf-8');
      setExportMessage(`CSV 已匯出至 ${path}`);
    } catch (error) { setExportMessage(`CSV 匯出失敗：${String(error)}`); }
    finally { setExporting(false); }
  }

  return <div className="report-workspace">
    <div className="workspace-toolbar">
      <h1>報表</h1>
      <button disabled={loading} onClick={() => setRefresh(value => value + 1)}>重新整理</button>
      <button disabled={loading || !report || exporting} onClick={() => void exportCsv()}>{exporting ? '匯出中…' : '匯出 CSV'}</button>
      {running && <span className="dim">正在計算新資料，目前顯示上次的報表</span>}
    </div>
    {exportMessage && <p role="status">{exportMessage}</p>}
    {loading && <p role="status">正在彙整報表…</p>}
    {error && <section className="report-section" role="alert"><h2>目前沒有可用報表</h2><p>{error}</p><p className="dim">請先在桌面版「計算」執行模擬，再回來檢視結果。</p></section>}
    {report && <>
      <section className="report-section">
        <h2>{report.scope.label} · Run #{report.scope.runId}</h2>
        <p>{report.scope.players} 人桌 · Hero 座位 {report.scope.heroSeat} · {report.scope.hands.toLocaleString()} 手 · {report.scope.instanceCount.toLocaleString()} 桌次</p>
        <p className="dim">策略 {report.scope.heroStrategy} · 基準 {report.scope.baselineVersion} · 引擎 {report.scope.engineVersion}</p>
        <p className="dim">Bot 組合 {report.scope.botPersonas.join(' / ')}</p>
        <p className="dim">建立時間 {new Date(report.scope.createdAt * 1000).toLocaleString()} · {report.scope.completed ? '已完成' : '已取消，僅含已完成的手牌'}</p>
        <p className="dim">統計主體為 Hero；工程基準尚未經顧問完整校準。</p>
      </section>
      <div className="report-grid">
        <section className="report-section report-metric"><h2>{report.overall.ev.verdict}</h2><Estimate ev={report.overall.ev} /><small>{report.overall.ev.estimator} · {report.overall.ev.effectiveBlocks} 個有效區塊</small><small>可分辨差距 ±{number(report.overall.ev.halfWidth)} bb/100</small></section>
        <section className="report-section report-metric"><h2>總盈虧</h2><strong className="num">{number(report.overall.netBb)} BB</strong><small>最大回撤 {number(report.overall.maxDrawdownBb)} BB</small><small>每手標準差 {number(report.overall.sigmaBb)} BB</small><small>每百手標準差 {number(report.overall.sigma100Bb)} BB</small></section>
        <section className="report-section report-metric"><h2>獲勝手數比例</h2><Rate rate={report.overall.handsWon} /><small>{report.overall.handsWon.estimator} · {report.overall.handsWon.effectiveClusters} 個有效桌次</small><small>平分 {report.overall.ties.toLocaleString()} 手</small></section>
        <section className="report-section report-metric"><h2>All-in EV</h2><strong>{report.overall.allInEvBb100 === null ? '尚無資料' : `${number(report.overall.allInEvBb100)} bb/100`}</strong><small>目前紀錄未保存分段底池 equity，無法估算 All-in EV。</small></section>
      </div>
      <section className="report-section">
        <div className="workspace-toolbar"><h2>逐位置統計</h2><label><input type="checkbox" checked={includeDead} onChange={event => setIncludeDead(event.target.checked)} /> 納入 dead button／dead small blind 手牌</label></div>
        <p className="dim">{includeDead ? '已納入 dead 手牌' : `已排除 ${report.deadHands.toLocaleString()} 手 dead 手牌`}，整體統計包含所有手牌。各列依當手在桌人數分開。</p>
        <div className="report-table-scroll"><table><thead><tr><th>桌型／位置</th><th>樣本手數</th><th>EV／95% CI</th><th>有效區塊／估計法</th><th>可分辨差距</th><th>獲勝手數</th><th>VPIP</th><th>PFR</th></tr></thead><tbody>
          {report.positions.map(row => <tr key={`${row.seated}-${row.position}`}><td>{row.seated}-max {row.position}</td><td className="num">{row.ev.hands.toLocaleString()}</td><td><Estimate ev={row.ev} /></td><td>{row.ev.effectiveBlocks}<small>{row.ev.estimator}</small></td><td className="num">±{number(row.ev.halfWidth)}</td><td><Rate rate={row.handsWon} /><small>{row.handsWon.estimator} · {row.handsWon.effectiveClusters} 桌次</small></td><td><Rate rate={row.vpip} /><small>{row.vpip.estimator} · {row.vpip.effectiveClusters} 桌次</small></td><td><Rate rate={row.pfr} /><small>{row.pfr.estimator} · {row.pfr.effectiveClusters} 桌次</small></td></tr>)}
          {!report.positions.length && <tr><td colSpan={8}>尚無可用的位置切片</td></tr>}
        </tbody></table></div>
      </section>
      <section className="report-section"><h2>行為頻率</h2><div className="report-table-scroll"><table><thead><tr><th>指標</th><th>比例</th><th>分子／分母</th><th>95% CI</th><th>估計法</th><th>有效桌次</th></tr></thead><tbody>{report.frequencies.map(rate => <tr key={rate.label}><td title={rate.definition}>{rate.label}<small>{rate.definition}</small></td><td className="num">{percent(rate.point)}</td><td className="num">{rate.numerator} / {rate.denominator}</td><td className="num">{interval(rate.ciLow, rate.ciHigh, true)}</td><td>{rate.estimator}</td><td className="num">{rate.effectiveClusters}</td></tr>)}</tbody></table></div></section>
      <div className="report-grid">
        <section className="report-section"><h2>桌次摘要</h2><p>{report.tables.instanceCount.toLocaleString()} 桌次 · 平均存活 {number(report.tables.avgHandsAlive)} 手 · 補位 {report.tables.refillCount.toLocaleString()} 次</p>{report.tables.endReasons.map(reason => <p key={reason.reason}>{reason.reason} · {reason.count.toLocaleString()} 次</p>)}</section>
        <section className="report-section"><h2>翻後策略執行覆蓋</h2>{report.postflopCoverage ? <><p>你的覆寫 {percent(report.postflopCoverage.completenessMyriad / 10_000)} · {report.postflopCoverage.user} / {report.postflopCoverage.totalDecisions} 次決策</p><p>官方 {report.postflopCoverage.official} · 同組通則 {report.postflopCoverage.generic} · 工程基準 {report.postflopCoverage.engineering}</p><p>未命中規則 {report.postflopCoverage.fallbackNoRule} · 合法動作被遮罩 {report.postflopCoverage.fallbackMasked}</p><small>節點集合 {report.postflopCoverage.nodeSetVersion}；工程基準不計入玩家完整度。</small></> : <p className="dim">此 run 沒有翻後覆蓋紀錄。</p>}</section>
      </div>
      <PreflopHitsCard reloadToken={reloadToken} />
    </>}
  </div>;
}
