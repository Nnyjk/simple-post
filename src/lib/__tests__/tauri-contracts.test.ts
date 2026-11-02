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

  it('以表驱动方式发送环境与模块地址的扁平字段', async () => {
    const cases: Array<[TauriCommand, Record<string, unknown>]> = [
      [COMMANDS.environmentsSetVar, { envId: 'env-1', key: 'token', value: 'abc' }],
      [COMMANDS.environmentsSetVar, { envId: 'env-1', key: 'token', value: null }],
      [
        COMMANDS.environmentsSetModuleBaseurl,
        {
          envId: 'env-1',
          moduleId: 'mod-1',
          url: 'https://example.test',
          proxy: 'https://gw.example.test',
        },
      ],
      [
        COMMANDS.environmentsSetModuleBaseurl,
        { envId: 'env-1', moduleId: 'mod-1', url: null, proxy: null },
      ],
      [COMMANDS.environmentsUpdate, { id: 'env-1', patch: { name: 'dev' } }],
      [COMMANDS.environmentsDelete, { id: 'env-1' }],
      // 树只有一种节点类型，所以移动就是父 id 加下标——没有 `parentKind`，
      // 目的端只有 `nodes` 一张表
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
