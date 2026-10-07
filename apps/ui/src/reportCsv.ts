import type { EstimateView, ProportionView, ReportView } from '../../../packages/poker-types/src/index';

type Cell = string | number | boolean | null | undefined;

/** CSV 字串欄位防止試算表把名稱或說明當成公式；數值負號保留。 */
export function csvCell(value: Cell): string {
  if (value === null || value === undefined || (typeof value === 'number' && !Number.isFinite(value))) return '';
  let text = String(value);
  if (typeof value === 'string' && /^[\s]*[=+\-@]/.test(value)) text = `'${text}`;
  return `"${text.replaceAll('"', '""')}"`;
}

export function reportCsv(report: ReportView): string {
  const rows: Cell[][] = [[
    'section', 'seated', 'position', 'metric', 'value', 'unit', 'numerator', 'denominator',
    'ci_low', 'ci_high', 'estimator', 'effective_samples', 'hands', 'verdict', 'definition',
  ]];
  const metric = (section: string, key: string, value: Cell, unit = '') =>
    rows.push([section, '', '', key, value, unit]);
  const estimate = (ev: EstimateView, seated?: number, position?: string) =>
    rows.push([position ? 'position' : 'overall', seated, position, 'EV', ev.point, 'bb/100', '', '',
      ev.ciLow, ev.ciHigh, ev.estimator, ev.effectiveBlocks, ev.hands, ev.verdict, `95% CI; halfWidth=${ev.halfWidth}`]);
  const proportion = (rate: ProportionView, section: string, seated?: number, position?: string) =>
    rows.push([section, seated, position, rate.label, rate.point === null ? null : rate.point * 100, '%',
      rate.numerator, rate.denominator, rate.ciLow === null ? null : rate.ciLow * 100,
      rate.ciHigh === null ? null : rate.ciHigh * 100, rate.estimator, rate.effectiveClusters,
      '', rate.denominator ? '' : '樣本不足', rate.definition]);
  for (const [key, value] of Object.entries(report.scope)) {
    metric('scope', key, Array.isArray(value) ? value.join(' / ') : value);
  }
  metric('scope', 'includeDead', report.includeDead);
  metric('scope', 'excludedDeadHands', report.includeDead ? 0 : report.deadHands, 'hands');
  estimate(report.overall.ev);
  for (const key of ['netBb', 'sigmaBb', 'sigma100Bb', 'maxDrawdownBb', 'allInEvBb100', 'ties'] as const) {
    metric('overall', key, report.overall[key], key === 'ties' ? 'hands' : key === 'allInEvBb100' ? 'bb/100' : 'bb');
  }
  proportion(report.overall.handsWon, 'overall'); proportion(report.overall.showdownWon, 'overall');
  for (const rate of report.frequencies) proportion(rate, 'frequency');
  for (const row of report.positions) {
    estimate(row.ev, row.seated, row.position);
    for (const rate of [row.handsWon, row.vpip, row.pfr]) proportion(rate, 'position', row.seated, row.position);
  }
  metric('tables', 'instanceCount', report.tables.instanceCount);
  metric('tables', 'avgHandsAlive', report.tables.avgHandsAlive, 'hands');
  metric('tables', 'refillCount', report.tables.refillCount);
  for (const reason of report.tables.endReasons) metric('tableEndReason', reason.reason, reason.count);
  if (report.postflopCoverage) {
    for (const [key, value] of Object.entries(report.postflopCoverage)) metric('postflopCoverage', key, value);
  }
  // UTF-8 BOM 讓 Excel 正確辨識繁體中文。每列欄位數一致。
  return '\uFEFF' + rows.map(row => Array.from({ length: 15 }, (_, i) => csvCell(row[i])).join(',')).join('\r\n') + '\r\n';
}
