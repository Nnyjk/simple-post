/**
 * Slice-level tests for `src/stores/mcp-slice.ts`.
 *
 * Targets from the second-pass audit (P2 tier):
 *   - T-1: `startMcpPolling` is idempotent + `visibilitychange` pauses
 *          and resumes the 3s `setInterval`.
 *   - T-1: `refreshMcpStatus` merges a successful Tauri invoke result
 *          into `mcpStatus`; on failure the last snapshot is preserved.
 *   - T-9: `restartMcp` optimistically flips `mcpStatus.status` to
 *          `'restarting'`; on IPC failure it flips back to `'crashed'`
 *          and records `lastError`.
 *
 * Run via:
 *   npx tsx --test src/stores/__tests__/mcp-slice.test.ts
 *
 * The store slices assume a Tauri webview environment
 * (`window.setInterval`, `document.visibilitychange`,
 * `window.__TAURI_INTERNALS__.invoke`); the `_browser-shim` helper
 * installs enough of a browser + IPC stub on `globalThis` for the
 * slice to run unchanged.
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

import {
  createMcpSlice,
  type McpSlice,
} from '../mcp-slice';
import type { McpUiState, RestartResult } from '@/lib/mcp-types';

// ---------- harness ----------

/**
 * Build a minimal "store" that the slice can be wired into. The slice's
 * `startMcpPolling` captures `state = get()` and references
 * `state.refreshMcpStatus` inside its tick — so we attach the slice
 * methods onto the very same object `get()` returns. That way the
 * polling pipeline is fully wired without bringing in the full
 * `useAppStore` (and all of bootstrap/ui/dirty-slice's transitive
 * module-load requirements).
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
  // Re-bind onto `bag` so `bag.refreshMcpStatus()` (called by the
  // ticking closure) and `bag.startMcpPolling()` resolve to the slice.
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
// T-1 — polling lifecycle
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
      // A fourth call, with timer still set, must not allocate a new timer.
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

      // Going hidden: handler should clear the timer and null the id.
      shim.setVisibility('hidden');
      assert.equal(mcp._mcpTimer, null, 'timer cleared on hidden');

      // Going visible: handler must restart polling and allocate a
      // fresh timer id.
      shim.setVisibility('visible');
      assert.notEqual(mcp._mcpTimer, null, 'timer restarted on visible');
      assert.notEqual(
        mcp._mcpTimer,
        initialTimer,
        'visible branch allocates a new timer id',
      );
      // Across the round-trip (start → hidden → visible) we should
      // have allocated exactly two timers.
      assert.equal(counter.count, 2, 'two setInterval calls total');
    } finally {
      mcp.stopMcpPolling();
    }
  });

  it('refreshMcpStatus merges success payload; failure keeps the last snapshot', async () => {
    // ---- success path ----
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

    // ---- failure path ----
    clearIpcMock();
    installIpcMock(async () => {
      throw new Error('daemon unreachable');
    });
    await mcp.refreshMcpStatus();
    assert.equal(mcp.mcpStatus.pid, previousPid, 'pid unchanged on failure');
    assert.equal(
      mcp.mcpStatus.uptimeMs,
      previousUptime,
      'uptime unchanged on failure',
    );
  });
});

// ============================================================
// T-9 — restartMcp failure recovery
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
    // The optimistic update is applied synchronously, before the
    // IPC round-trip resolves.
    assert.equal(
      mcp.mcpStatus!.status,
      'restarting',
      'status optimistically flipped before await',
    );
    await inflight;
    // After the daemon-restart succeeds and refreshMcpStatus pulls the
    // fresh snapshot, status should be the polled `running`.
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
    assert.equal(
      mcp.mcpStatus!.status,
      'crashed',
      'failure path flips status back to crashed',
    );
    assert.equal(
      mcp.mcpStatus!.lastError,
      'daemon down',
      'lastError captures the thrown message',
    );
  });
});
