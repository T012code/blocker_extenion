import { test } from 'node:test';
import assert from 'node:assert/strict';
import { action, bytes, eta, percent, speed } from '../src/shared/format.js';

test('formats sizes, speeds and time left', () => {
  assert.equal(bytes(0), '0 B');
  assert.equal(bytes(1536), '1.5 KB');
  assert.equal(bytes(250 * 1024 * 1024), '250 MB');
  assert.equal(bytes(null), '?');
  assert.equal(speed(0), '');
  assert.equal(speed(2 * 1024 * 1024), '2 MB/s');
  assert.equal(bytes(12 * 1024 * 1024), '12 MB');
  assert.equal(eta(100, 10), '10 s left');
  assert.equal(eta(10 * 60 * 1000, 1000), '10 min left');
  assert.equal(eta(100, 0), '');
  assert.equal(percent(50, 200), 25);
  assert.equal(percent(5, null), null);
});

test('pause while running, continue when stopped', () => {
  assert.equal(action('downloading'), 'pause');
  assert.equal(action('connecting'), 'pause');
  for (const s of ['paused', 'failed', 'queued', 'needsRefresh']) assert.equal(action(s), 'continue');
  assert.equal(action('completed'), null);
});
