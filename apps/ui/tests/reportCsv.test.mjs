import { test } from 'node:test';
import assert from 'node:assert/strict';
import { csvCell, reportCsv } from '../src/reportCsv.ts';

test('CSV escapes quotes, commas, newlines and spreadsheet formulas without changing numeric losses', () => {
  assert.equal(csvCell('策略,"A"\n下一行'), '"策略,""A""\n下一行"');
  assert.equal(csvCell('=1+1'), '"\'=1+1"');
  assert.equal(csvCell(' @SUM(A1)'), '"\' @SUM(A1)"');
  assert.equal(csvCell(-1.5), '"-1.5"');
  assert.equal(csvCell(null), '');
  assert.equal(csvCell(Infinity), '');
});

test('CSV includes inference context, slice filters, sample counts, and missing all-in equity', () => {
  const rate = { label: 'VPIP', point: null, numerator: 0, denominator: 0, ciLow: null, ciHigh: null, estimator: 'Wilson', effectiveClusters: 0, definition: 'Voluntary entry' };
  const ev = { point: -3.5, ciLow: -10, ciHigh: 3, halfWidth: 6.5, estimator: 'block-bootstrap', effectiveBlocks: 20, hands: 500, verdict: '本樣本無法判定優劣' };
  const report = {
    scope: { runId: 9, heroStrategy: '=untrusted', baselineVersion: 'v1', hands: 500 },
    includeDead: false, deadHands: 5,
    overall: { ev, netBb: -17.5, sigmaBb: 2, sigma100Bb: 20, maxDrawdownBb: 50, allInEvBb100: null, ties: 2, handsWon: rate, showdownWon: rate },
    frequencies: [rate], positions: [{ seated: 6, position: 'BTN', ev, handsWon: rate, vpip: rate, pfr: rate }],
    tables: { instanceCount: 2, avgHandsAlive: 250, refillCount: 0, endReasons: [{ reason: 'handLimit', count: 2 }] },
    postflopCoverage: null,
  };
  const csv = reportCsv(report);
  assert.ok(csv.startsWith('\uFEFF'));
  assert.ok(csv.endsWith('\r\n'));
  assert.ok(csv.includes('"\'=untrusted"'));
  assert.ok(csv.includes('"position","6","BTN","EV","-3.5","bb/100"'));
  assert.ok(csv.includes('"block-bootstrap","20","500","本樣本無法判定優劣"'));
  assert.ok(csv.includes('"allInEvBb100",,"bb/100"'));
  assert.ok(csv.includes('"excludedDeadHands","5","hands"'));
});
