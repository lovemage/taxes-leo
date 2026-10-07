import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyStrategy, parseStrategy, readLibrary, strategyContent, strategyNameError } from '../src/strategyLibrary.ts';

const request = {
  heroSeat: 1, players: 9, masterSeed: '12345678901234567890', bigBlind: 2,
  bots: [{ name: 'other', params: { rangeWidth: 12000 } }, { name: 'Hero', params: { openSizeCentiBb: 450 } }],
  heroOverrides: [{ seated: 9, hero: 'BTN', bucket: '160-240', scenario: 'unopened', class: 'AA', aggressive: 9000, call: 1000 }],
  heroPostflopOverrides: { nodes: [], rules: [] },
  heroOpenTiers: { mediumAboveCentiBb: 300, largeAboveCentiBb: 600 },
};
const saved = {
  format: '9max-strategy', schemaVersion: 1, id: 'one', name: '策略 A', version: 2,
  baselineVersion: 'baseline/v1', updatedAt: '2026-10-06T00:00:00Z', warnings: [],
  content: strategyContent(request),
};

test('portable strategies restore Hero content without changing table, seed, or opponents', () => {
  const target = { ...request, heroSeat: 0, players: 6, masterSeed: '88', bots: [{ name: 'old', params: {} }, { name: 'opponent', params: {} }] };
  const restored = applyStrategy(target, saved.content);
  assert.equal(restored.heroSeat, 0);
  assert.equal(restored.players, 6);
  assert.equal(restored.masterSeed, '88');
  assert.deepEqual(restored.bots[1], target.bots[1]);
  assert.deepEqual(restored.bots[0], request.bots[1]);
  restored.heroOverrides[0].aggressive = 0;
  assert.equal(saved.content.heroOverrides[0].aggressive, 9000);
});

test('loading default Hero settings clears parameters left by the previous strategy', () => {
  const restored = applyStrategy(request, { ...saved.content, heroBot: null });
  assert.deepEqual(restored.bots[1].params, {});
});

test('names cannot silently overwrite another strategy', () => {
  assert.equal(strategyNameError('策略 A', [saved], 'one'), null);
  assert.ok(strategyNameError(' 策略 A ', [saved]));
  assert.ok(strategyNameError('  ', []));
  assert.ok(strategyNameError('字'.repeat(61), []));
});

test('unsupported or corrupt imports and duplicate library identities are rejected', () => {
  assert.deepEqual(parseStrategy(JSON.stringify(saved)), saved);
  assert.throws(() => parseStrategy(JSON.stringify({ ...saved, schemaVersion: 2 })));
  assert.throws(() => parseStrategy(JSON.stringify({ ...saved, content: {} })));
  assert.throws(() => readLibrary({ getItem: () => JSON.stringify([saved, { ...saved, name: 'different' }]) }));
});
