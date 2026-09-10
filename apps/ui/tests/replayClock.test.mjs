import { test } from 'node:test';
import assert from 'node:assert/strict';
import { advanceTime, frameDuration } from '../src/components/replayClock.ts';
test('pause preserves presentation time and resume respects speed', () => {
  assert.equal(advanceTime(350, 50, 2, false), 350);
  assert.equal(advanceTime(350, 50, 2, true), 450);
  assert.equal(advanceTime(350, 50, .5, true), 375);
});
test('returning from a background tab cannot skip a settlement', () => {
  assert.equal(advanceTime(0, 10000, 1, true), 100);
  assert.equal(frameDuration('settle'), 2000);
  assert.ok(frameDuration('dealHole') >= 9 * 2 * 100 + 350);
});
