import { test } from 'node:test';
import assert from 'node:assert/strict';
import { daemonTone } from '@/lib/mcp-status-tone';
import type { McpRuntimeStatus } from '@/lib/mcp-types';

/**
 * 状态卡片历史实现曾固定绑定 `concept="connection"`（玫瑰色），而邻近徽标
 * 显示实际生命周期。此处固化映射关系：状态色调异常不会报错，但会导致运行正常的
 * 守护进程被标记为红色。
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
  // 守护进程在版本构建后新增的状态枚举字符串。
  assert.equal(daemonTone('degraded' as McpRuntimeStatus), 'default');
});

test('no lifecycle paints the rose that started this', () => {
  // `connection` 色相固定为 355°（`--hue-connection`）。上述状态映射不可再引入该色相；
  // 设置侧边栏将其用作导航概念标识，职责不同。
  for (const s of ALL) {
    assert.notEqual(daemonTone(s), 'connection' as unknown as ReturnType<typeof daemonTone>);
  }
});
