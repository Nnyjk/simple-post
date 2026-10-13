/**
 * Tests for the persisted half of `src/stores/slices/ui-response.ts` —
 * `addResponseHistory` / `loadResponseHistory` / `clearResponseHistory`.
 *
 * These run against the **real composed store**, same as
 * `ui-draft-endpoints.test.ts`: the contract they protect is a contract
 * *across* slices (`setActiveProject` is what triggers the load) and
 * *with* the IPC boundary, and a hand-built harness would stub away the
 * half most likely to break.
 *
 * The interesting assertions are all about things a caller gets wrong
 * silently:
 *   - a `draft:` endpoint is never written, because it has no `nodes` row
 *     for the foreign key to hold the row against;
 *   - the provisional row is *replaced* by the server's, because only the
 *     stored entry knows the body was clipped;
 *   - a failed append leaves the row in place and does not reject —
 *     the send already succeeded, and a history write is not a send;
 *   - a load that lands after the user switched projects is dropped.
 *
 * Run via: npx tsx --test src/stores/__tests__/ui-response-history.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { DRAFT_ID_PREFIX } from '../store-types';
import type { ApiResponse, ResponseHistoryEntry } from '@/types/domain';

type IpcCall = { cmd: string; args: Record<string, unknown> };

const REAL_ID = 'ep1';
const DRAFT_ID = `${DRAFT_ID_PREFIX}new1`;

function makeResponse(over: Partial<ApiResponse> = {}): ApiResponse {
  return {
    status: 200,
    statusText: 'OK',
    durationMs: 12,
    sizeBytes: 2,
    headers: {},
    body: 'hi',
    contentType: 'text/plain',
    method: 'GET',
    url: '/x',
    ...over,
  };
}

function makeEntry(endpointId: string, id: string, over: Partial<ResponseHistoryEntry> = {}) {
  return {
    id,
    endpointId,
    response: makeResponse(),
    bodyTruncated: false,
    createdAt: 1,
    ...over,
  } satisfies ResponseHistoryEntry;
}

/** Every IPC call the store made, in order. The "did it write?" oracle. */
let calls: IpcCall[] = [];
/** What `response_history_append` resolves to, when it resolves at all. */
let appendResult: ResponseHistoryEntry | null = null;
/** Make `response_history_append` reject, the way a daemon failure would. */
let appendFails = false;
/** Gate the list command so a load can be caught mid-flight. */
let listGate: Promise<void> | null = null;
/** What `response_history_list` resolves to. */
let listResult: ResponseHistoryEntry[] = [];
/** Make `response_history_list` reject. */
let listFails = false;

let origWarn: typeof console.warn;

beforeEach(() => {
  installBrowserShim();
  calls = [];
  appendResult = null;
  appendFails = false;
  listGate = null;
  listResult = [];
  listFails = false;
  // The store logs (not throws) on both failure paths; silence it so the
  // test output only shows real failures.
  origWarn = console.warn.bind(console);
  console.warn = () => {};

  installIpcMock(async (cmd, args) => {
    calls.push({ cmd, args: args ?? {} });
    if (cmd === 'response_history_append') {
      if (appendFails) throw new Error('append failed');
      // A real command always returns the row it stored. Falling back to
      // echoing the request keeps a test that forgets to set
      // `appendResult` honest rather than crashing on undefined.
      return appendResult ?? makeEntry(args.endpointId, 'rh-fallback');
    }
    if (cmd === 'response_history_list') {
      if (listGate) await listGate;
      if (listFails) throw new Error('list failed');
      return listResult;
    }
    if (cmd === 'response_history_clear') return 1;
    throw new Error(`unexpected command: ${cmd}`);
  });

  useAppStore.setState({ responseHistory: {}, activeProjectId: '' });
});

afterEach(() => {
  console.warn = origWarn;
  clearIpcMock();
});

const cmdNames = () => calls.map((c) => c.cmd);

describe('addResponseHistory 的落盘与乐观行', () => {
  it('真实接口发一次，只写一次；draft 接口不写', () => {
    const { addResponseHistory } = useAppStore.getState();

    addResponseHistory(REAL_ID, makeResponse());
    addResponseHistory(DRAFT_ID, makeResponse());

    assert.deepEqual(
      cmdNames(),
      ['response_history_append'],
      'a draft endpoint has no `nodes` row, so its history is memory-only',
    );
    assert.deepEqual(calls[0].args, {
      endpointId: REAL_ID,
      response: makeResponse(),
    });
    // Both still accumulate locally — that is what the strip renders.
    assert.equal(useAppStore.getState().responseHistory[REAL_ID]?.length, 1);
    assert.equal(useAppStore.getState().responseHistory[DRAFT_ID]?.length, 1);
  });

  it('落盘失败时保留乐观行，且不抛出', async () => {
    appendFails = true;
    const { addResponseHistory } = useAppStore.getState();

    assert.doesNotThrow(() => addResponseHistory(REAL_ID, makeResponse()));
    const before = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.equal(before.length, 1, 'the row is there before the IPC is even awaited');

    // Drain the fire-and-forget promise. An unhandled rejection here
    // would fail the run rather than the assertion.
    await new Promise((resolve) => setTimeout(resolve, 0));

    const after = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.deepEqual(after, before, 'a failed history write must not erase what the user can see');
  });

  it('服务端返回的行替换乐观行（截断信息只有它有）', async () => {
    const stored = makeEntry(REAL_ID, 'rh-1', {
      response: makeResponse({ body: 'clipped…', bodyJson: undefined }),
      bodyTruncated: true,
      createdAt: 1_700_000_000_000,
    });
    appendResult = stored;

    useAppStore.getState().addResponseHistory(REAL_ID, makeResponse({ body: 'the whole thing' }));
    await new Promise((resolve) => setTimeout(resolve, 0));

    const rows = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.equal(rows.length, 1, 'replaced in place, not appended to');
    assert.deepEqual(rows[0], stored);
    assert.equal(rows[0].bodyTruncated, true);
    assert.equal(
      rows[0].response.bodyJson,
      undefined,
      'a clipped row carries no pretty-printed copy',
    );
  });

  it('两条同时在途的落盘各替换自己的乐观行', async () => {
    // Distinct ids per call, and the second append resolves *first* — the
    // shape that breaks any implementation keying the swap on something
    // weaker than the provisional row's own id.
    let n = 0;
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'response_history_append') {
        n += 1;
        const id = `rh-${n}`;
        await new Promise((resolve) => setTimeout(resolve, n === 2 ? 0 : 5));
        // Echo the payload the way the real command does, so a crossed
        // swap shows up as a wrong status rather than two identical rows.
        return makeEntry(args.endpointId, id, { response: args.response as ApiResponse });
      }
      throw new Error(`unexpected command: ${cmd}`);
    });

    const { addResponseHistory } = useAppStore.getState();
    addResponseHistory(REAL_ID, makeResponse({ status: 201 }));
    addResponseHistory(REAL_ID, makeResponse({ status: 202 }));
    await new Promise((resolve) => setTimeout(resolve, 30));

    const rows = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.equal(rows.length, 2, 'neither provisional row was lost or duplicated');
    assert.deepEqual(
      rows.map((r) => [r.id, r.response.status]),
      [
        ['rh-1', 201],
        ['rh-2', 202],
      ],
      'each row keeps its own payload; the out-of-order resolve did not cross them',
    );
  });

  it('内存 FIFO 上限仍是 10，落盘不改变它', () => {
    appendFails = true; // keep the store-side append on the optimistic path
    const { addResponseHistory } = useAppStore.getState();
    for (let i = 0; i < 12; i++) {
      addResponseHistory(REAL_ID, makeResponse({ status: 200 + i }));
    }
    const rows = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.equal(rows.length, 10, 'the strip renders at most ten');
    assert.deepEqual(
      rows.map((r) => r.response.status),
      [202, 203, 204, 205, 206, 207, 208, 209, 210, 211],
      'the two oldest are dropped, not the two newest',
    );
  });
});

describe('loadResponseHistory 的分组与竞态', () => {
  it('按 endpointId 分组进 Record', async () => {
    listResult = [makeEntry('epA', 'a2'), makeEntry('epB', 'b1'), makeEntry('epA', 'a1')];
    // The commit is guarded on the active project still being `rootId`,
    // which in the app is true because `setActiveProject` set it before
    // dispatching. Mirror that here.
    useAppStore.setState({ activeProjectId: 'p1' });
    await useAppStore.getState().loadResponseHistory('p1');

    assert.deepEqual(
      Object.keys(useAppStore.getState().responseHistory).sort(),
      ['epA', 'epB'],
      'the flat list becomes one key per endpoint',
    );
    assert.deepEqual(
      useAppStore.getState().responseHistory.epA?.map((r) => r.id),
      ['a2', 'a1'],
      'server order is preserved within an endpoint (newest first)',
    );
  });

  it('加载失败不抛出，也不写 bootstrapError', async () => {
    listFails = true;
    await assert.doesNotReject(() => useAppStore.getState().loadResponseHistory('p1'));
    assert.equal(useAppStore.getState().bootstrapError, null);
    assert.deepEqual(useAppStore.getState().responseHistory, {});
  });

  it("rootId 为 '' 时不发请求", async () => {
    await useAppStore.getState().loadResponseHistory('');
    assert.deepEqual(cmdNames(), [], "'' is the welcome screen, not a project");
  });

  it('中途切换项目后，迟到的结果被丢弃', async () => {
    let release: () => void = () => undefined;
    listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    listResult = [makeEntry('epA', 'a1')];

    useAppStore.setState({ activeProjectId: 'p1' });
    const loading = useAppStore.getState().loadResponseHistory('p1');

    // The user leaves for another project while the read is in flight.
    useAppStore.setState({ activeProjectId: 'p2' });
    release();
    await loading;

    assert.deepEqual(
      useAppStore.getState().responseHistory,
      {},
      "p1's rows must not land under p2",
    );
  });

  it('切换项目重新加载：setActiveProject 是唯一的触发点', async () => {
    listResult = [makeEntry('epA', 'a1')];

    useAppStore.getState().setActiveProject('p1');
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(cmdNames(), ['response_history_list']);
    assert.deepEqual(calls[0].args, { rootId: 'p1' });
    assert.deepEqual(Object.keys(useAppStore.getState().responseHistory), ['epA']);

    // Re-selecting the project already in front is not an event.
    useAppStore.getState().setActiveProject('p1');
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(cmdNames(), ['response_history_list'], 'no re-fetch for the same project');
  });

  it('新项目的加载整体替换旧项目的 map', async () => {
    listResult = [makeEntry('epA', 'a1')];
    useAppStore.setState({ activeProjectId: 'p1' });
    await useAppStore.getState().loadResponseHistory('p1');
    assert.deepEqual(Object.keys(useAppStore.getState().responseHistory), ['epA']);

    listResult = [makeEntry('epZ', 'z1')];
    useAppStore.setState({ activeProjectId: 'p2' });
    await useAppStore.getState().loadResponseHistory('p2');
    assert.deepEqual(
      Object.keys(useAppStore.getState().responseHistory),
      ['epZ'],
      "p1's keys do not survive the switch",
    );
  });
});

describe('clearResponseHistory', () => {
  it('乐观清空并落一次库；draft 不落盘', async () => {
    appendFails = true;
    const { addResponseHistory, clearResponseHistory } = useAppStore.getState();
    addResponseHistory(REAL_ID, makeResponse());
    addResponseHistory(DRAFT_ID, makeResponse());
    calls = [];

    await clearResponseHistory(REAL_ID);
    await clearResponseHistory(DRAFT_ID);

    assert.deepEqual(cmdNames(), ['response_history_clear']);
    assert.deepEqual(calls[0].args, { endpointId: REAL_ID });
    assert.ok(
      !(REAL_ID in useAppStore.getState().responseHistory),
      'the strip must not keep rows the file may no longer have',
    );
    // A draft's rows are memory-only, so clearing is purely local.
    assert.ok(!(DRAFT_ID in useAppStore.getState().responseHistory));
  });

  it('清空失败不抛，也不把行放回去', async () => {
    appendFails = true;
    const { addResponseHistory, clearResponseHistory } = useAppStore.getState();
    addResponseHistory(REAL_ID, makeResponse());
    installIpcMock(async (cmd) => {
      calls.push({ cmd, args: {} });
      if (cmd === 'response_history_clear') throw new Error('clear failed');
      throw new Error(`unexpected command: ${cmd}`);
    });

    await assert.doesNotReject(() => clearResponseHistory(REAL_ID));
    assert.ok(!(REAL_ID in useAppStore.getState().responseHistory));
  });
});
