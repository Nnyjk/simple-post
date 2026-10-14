/**
 * `src/stores/mcp-slice.ts` 的切片级测试。
 *
 * 覆盖的职责：
 *   - `startMcpPolling` 幂等，且 `visibilitychange` 会暂停并恢复那个 3s
 *     `setInterval`。
 *   - `refreshMcpStatus` 把一次成功的 Tauri invoke 结果并入 `mcpStatus`；
 *     失败时保留上一次快照。
 *   - `restartMcp` 乐观地把 `mcpStatus.status` 翻成 `'restarting'`；IPC 失败时
 *     翻回 `'crashed'` 并记下 `lastError`。
 *
 * 运行：
 *   npx tsx --test src/stores/__tests__/mcp-slice.test.ts
 *
 * store 各切片假设自己跑在 Tauri webview 环境里（`window.setInterval`、
 * `document.visibilitychange`、`window.__TAURI_INTERNALS__.invoke`）；`_browser-shim`
 * 在 `globalThis` 上装够用的浏览器 + IPC 桩，让切片能原样运行。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import {
  installBrowserShim,
  installIpcMock,
  clearIpcMock,
  SetIntervalCounter,
  type ShimGlobals,
} from './_browser-shim';

import { createMcpSlice, type McpSlice } from '../mcp-slice';
import type { McpUiState, RestartResult } from '@/lib/mcp-types';

// ---------- harness ----------

/**
 * 搭一个能把这个切片接进去的最小"store"。切片的 `startMcpPolling` 会捕获
 * `state = get()`，并在 tick 里引用 `state.refreshMcpStatus`——所以把切片方法
 * 挂在 `get()` 返回的**同一个**对象上。这样轮询管线完整接通，又不必拖进整个
 * `useAppStore`（以及 bootstrap/ui/dirty 各切片带来的全部传递性模块加载要求）。
 */
function makeMcpHarness(initialStatus: McpUiState | null = null) {
  const bag: Record<string, any> = {
    mcpStatus: initialStatus,
    _mcpTimer: null,
    restartDialogSeen: null,
  };
  const set = (partial: any) => {
    const updater = typeof partial === 'function' ? partial(bag) : partial;
    if (updater) Object.assign(bag, updater);
  };
  const get = () => bag;

  const slice = createMcpSlice(set as any, get as any, {} as any);
  // 重新绑到 `bag` 上，好让 `bag.refreshMcpStatus()`（由 tick 闭包调用）与
  // `bag.startMcpPolling()` 都解析到切片。
  Object.assign(bag, slice);
  return bag as unknown as McpSlice;
}

const BASE_RUNNING: McpUiState = {
  status: 'running',
  running: true,
  pid: 1234,
  port: 4567,
  uptimeMs: 5_000,
  requestsHandled: 0,
  currentConnections: 0,
  registeredTools: [],
};

let shim: ShimGlobals;
let counter: SetIntervalCounter;

beforeEach(() => {
  shim = installBrowserShim();
  counter = new SetIntervalCounter();
  counter.install();
});

afterEach(() => {
  shim.teardown();
  counter.restore();
  clearIpcMock();
});

// ============================================================
// T-1 —— 轮询生命周期
// ============================================================

describe('mcp-slice: startMcpPolling — T-1 idempotency + visibility', () => {
  it('is idempotent — repeated calls yield exactly one setInterval', () => {
    const mcp = makeMcpHarness();
    mcp.startMcpPolling();
    mcp.startMcpPolling();
    mcp.startMcpPolling();
    try {
      assert.equal(
        counter.count,
        1,
        'only one setInterval should be installed across N start() calls',
      );
      assert.notEqual(mcp._mcpTimer, null, 'timer id persisted on store');
      const firstId = mcp._mcpTimer;
      // 第四次调用时定时器仍在，绝不能再分配一个新定时器。
      mcp.startMcpPolling();
      assert.equal(mcp._mcpTimer, firstId, 'idempotent — same timer id');
      assert.equal(counter.count, 1, 'no second setInterval registered');
    } finally {
      mcp.stopMcpPolling();
    }
  });

  it('visibilitychange to "hidden" clears the timer; to "visible" restarts it', () => {
    const mcp = makeMcpHarness();
    try {
      mcp.startMcpPolling();
      const initialTimer = mcp._mcpTimer;
      assert.notEqual(initialTimer, null, 'precondition: timer running');

      // 切到隐藏：handler 应当清掉定时器并把 id 置空。
      shim.setVisibility('hidden');
      assert.equal(mcp._mcpTimer, null, 'timer cleared on hidden');

      // 切回可见：handler 必须重启轮询并分配一个新的定时器 id。
      shim.setVisibility('visible');
      assert.notEqual(mcp._mcpTimer, null, 'timer restarted on visible');
      assert.notEqual(mcp._mcpTimer, initialTimer, 'visible branch allocates a new timer id');
      // 整轮往返（start → 隐藏 → 可见）下来，应当恰好分配过两个定时器。
      assert.equal(counter.count, 2, 'two setInterval calls total');
    } finally {
      mcp.stopMcpPolling();
    }
  });

  it('refreshMcpStatus merges success payload; failure keeps the last snapshot', async () => {
    // ---- 成功路径 ----
    installIpcMock(async (cmd) => {
      if (cmd === 'mcp_status') {
        return { ...BASE_RUNNING, pid: 9999, requestsHandled: 42 };
      }
      throw new Error('unexpected cmd: ' + cmd);
    });
    const mcp = makeMcpHarness();
    await mcp.refreshMcpStatus();
    assert.ok(mcp.mcpStatus, 'mcpStatus populated on success');
    assert.equal(mcp.mcpStatus.status, 'running');
    assert.equal(mcp.mcpStatus.pid, 9999, 'success payload merged');
    assert.equal(mcp.mcpStatus.requestsHandled, 42);

    const previousPid = mcp.mcpStatus.pid;
    const previousUptime = mcp.mcpStatus.uptimeMs;

    // ---- 失败路径 ----
    clearIpcMock();
    installIpcMock(async () => {
      throw new Error('daemon unreachable');
    });
    await mcp.refreshMcpStatus();
    assert.equal(mcp.mcpStatus.pid, previousPid, 'pid unchanged on failure');
    assert.equal(mcp.mcpStatus.uptimeMs, previousUptime, 'uptime unchanged on failure');
  });
});

// ============================================================
// T-9 —— restartMcp 失败恢复
// ============================================================

describe('mcp-slice: restartMcp — T-9 failure recovery', () => {
  it('optimistically flips status to "restarting" then resolves to running', async () => {
    const restartResponse: RestartResult = { ok: true, pid: 5555, port: 6789 };
    const polledStatus: McpUiState = {
      ...BASE_RUNNING,
      pid: 5555,
      port: 6789,
      status: 'running',
    };
    installIpcMock(async (cmd) => {
      if (cmd === 'mcp_daemon_restart') return restartResponse;
      if (cmd === 'mcp_status') return polledStatus;
      throw new Error('unexpected cmd: ' + cmd);
    });
    const mcp = makeMcpHarness({ ...BASE_RUNNING, status: 'crashed' });

    const inflight = mcp.restartMcp();
    // 乐观更新在 IPC 往返 resolve 之前就同步生效了。
    assert.equal(mcp.mcpStatus!.status, 'restarting', 'status optimistically flipped before await');
    await inflight;
    // 守护进程重启成功、refreshMcpStatus 拉到新快照之后，status 应当是轮询到的
    // `running`。
    assert.equal(mcp.mcpStatus!.status, 'running');
    assert.equal(mcp.mcpStatus!.pid, 5555);
    assert.equal(mcp.mcpStatus!.port, 6789);
  });

  it('on ipc failure flips status to "crashed" and records lastError', async () => {
    installIpcMock(async (cmd) => {
      if (cmd === 'mcp_daemon_restart') {
        throw new Error('daemon down');
      }
      throw new Error('unexpected cmd: ' + cmd);
    });
    const mcp = makeMcpHarness({ ...BASE_RUNNING, status: 'running' });

    await assert.rejects(
      () => mcp.restartMcp(),
      /daemon down/,
      'restartMcp should re-throw the original error',
    );
    assert.equal(mcp.mcpStatus!.status, 'crashed', 'failure path flips status back to crashed');
    assert.equal(mcp.mcpStatus!.lastError, 'daemon down', 'lastError captures the thrown message');
  });
});
