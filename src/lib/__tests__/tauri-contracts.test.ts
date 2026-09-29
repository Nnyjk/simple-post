/**
 * Tauri IPC 契约测试：确认渲染层发送的字段名与 Rust 命令参数一致。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { clearIpcMock, installBrowserShim, installIpcMock } from '@/stores/__tests__/_browser-shim';
import { COMMANDS, invokeCmd, type TauriCommand } from '../tauri-commands';

interface CapturedCall {
  cmd: string;
  args: Record<string, unknown>;
}

describe('Tauri IPC 入参契约', () => {
  let calls: CapturedCall[] = [];

  beforeEach(() => {
    installBrowserShim();
    calls = [];
  });

  afterEach(() => {
    clearIpcMock();
  });

  it('以表驱动方式发送环境与 BaseUrl 的扁平字段', async () => {
    const cases: Array<[TauriCommand, Record<string, unknown>]> = [
      [COMMANDS.environmentsSetVar, { envId: 'env-1', key: 'token', value: 'abc' }],
      [COMMANDS.environmentsSetVar, { envId: 'env-1', key: 'token', value: null }],
      [
        COMMANDS.environmentsSetBaseurl,
        { envId: 'env-1', defId: 'def-1', url: 'https://example.test' },
      ],
      [COMMANDS.environmentsSetBaseurl, { envId: 'env-1', defId: 'def-1', url: null }],
      [COMMANDS.environmentsUpdate, { id: 'env-1', patch: { name: 'dev' } }],
      [COMMANDS.environmentsDelete, { id: 'env-1' }],
      [COMMANDS.baseUrlDefsUpdate, { id: 'def-1', patch: { name: 'api' } }],
      [COMMANDS.baseUrlDefsDelete, { id: 'def-1' }],
      // The tree is one node type now, so a move is a parent id and an
      // index — no `parentKind`, which only ever existed because the
      // destination could live in one of two tables.
      [COMMANDS.nodesMove, { nodeIds: ['c1'], parentId: 'm1', index: 0 }],
    ];

    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
      return null;
    });

    for (const [cmd, args] of cases) {
      await invokeCmd(cmd, args);
    }

    assert.deepEqual(
      calls,
      cases.map(([cmd, args]) => ({ cmd, args })),
    );
  });

  it('能力策略 hydration 在 Tauri 形状返回 { policy }', async () => {
    installIpcMock(async (cmd) => {
      if (cmd === COMMANDS.mcpCapabilityGet) return { policy: 'safe_write' };
      throw new Error(`unexpected command: ${cmd}`);
    });

    const result = await invokeCmd<{ policy: 'read_only' | 'safe_write' | 'full_access' }>(
      COMMANDS.mcpCapabilityGet,
    );
    assert.deepEqual(result, { policy: 'safe_write' });
  });
});
