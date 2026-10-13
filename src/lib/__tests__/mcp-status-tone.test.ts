import { test } from 'node:test';
import assert from 'node:assert/strict';
import { daemonTone } from '@/lib/mcp-status-tone';
import type { McpRuntimeStatus } from '@/lib/mcp-types';

/**
 * The hero tile used to be `concept="connection"` — a fixed rose — while
 * the badge next to it reported the real lifecycle. These lock the mapping
 * down, because the failure mode is silent: a wrong tone does not throw,
 * it just paints a healthy daemon red and trusts the reader to notice.
 */
const ALL: readonly McpRuntimeStatus[] = [
  'stopped',
  'starting',
  'running',
  'restarting',
  'crashed',
  'unhealthy',
];

test('running is the only success tone', () => {
  assert.equal(daemonTone('running'), 'success');
  for (const s of ALL) {
    if (s !== 'running') assert.notEqual(daemonTone(s), 'success', `${s} must not read as healthy`);
  }
});

test('in-flight lifecycles are warning, failures are danger', () => {
  assert.equal(daemonTone('starting'), 'warning');
  assert.equal(daemonTone('restarting'), 'warning');
  assert.equal(daemonTone('crashed'), 'danger');
  assert.equal(daemonTone('unhealthy'), 'danger');
});

test('stopped is neutral — not running is not an error', () => {
  assert.equal(daemonTone('stopped'), 'default');
});

test('unknown / not-yet-polled states collapse to neutral', () => {
  assert.equal(daemonTone(null), 'default');
  assert.equal(daemonTone(undefined), 'default');
  // A string the daemon invents after this build shipped.
  assert.equal(daemonTone('degraded' as McpRuntimeStatus), 'default');
});

test('no lifecycle paints the rose that started this', () => {
  // `connection`'s hue is 355° (`--hue-connection`). The mapping above
  // must never re-introduce it as a *status*; the settings rail still
  // uses the concept for navigation identity, which is a different job.
  for (const s of ALL) {
    assert.notEqual(daemonTone(s), 'connection' as unknown as ReturnType<typeof daemonTone>);
  }
});
