/**
 * `src/stores/slices/ui-response.ts` 落盘那一半的测试——`addResponseHistory` /
 * `loadResponseHistory` / `clearResponseHistory`。
 *
 * 与 `ui-draft-endpoints.test.ts` 一样跑在**真实组合 store** 上：它们保护的是
 * **跨**切片的契约（`setActiveProject` 才是触发加载的那个）**并**跨越 IPC 边界，
 * 手搭的 harness 恰好会把最容易坏的那一半 stub 掉。
 *
 * 有意思的断言都关于调用方会悄无声息搞错的事：
 *   - `draft:` endpoint 永不写入，因为它没有 `nodes` 行给外键挂这一行；
 *   - 乐观行被服务端的行**替换**，因为只有已存那条知道 body 被截断过；
 *   - 写入失败时行留在原地且不 reject——发送已经成功了，历史写入不是发送；
 *   - 读取是合并而非覆盖：它是用户按下 Send **之前**的快照，所以对那次发送
 *     一无所知；store 的 newest-last 才是 `appendCapped` 与历史条共同的次序，
 *     而不是回包的 `created_at DESC`。
 *
 * 运行：npx tsx --test src/stores/__tests__/ui-response-history.test.ts
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

/** store 发出过的每次 IPC 调用，按顺序。"写没写？"的裁判。 */
let calls: IpcCall[] = [];
/** `response_history_append` resolve 成什么（如果它 resolve 的话）。 */
let appendResult: ResponseHistoryEntry | null = null;
/** 让 `response_history_append` reject，就像守护进程失败那样。 */
let appendFails = false;
/** 给 list 命令设闸，好让一次加载能在半途被抓住。 */
let listGate: Promise<void> | null = null;
/** `response_history_list` resolve 成什么。 */
let listResult: ResponseHistoryEntry[] = [];
/** 让 `response_history_list` reject。 */
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
  // store 在两条失败路径上都是记日志而不是抛出；静音它，好让测试输出只显示真失败。
  origWarn = console.warn.bind(console);
  console.warn = () => {};

  installIpcMock(async (cmd, args) => {
    calls.push({ cmd, args: args ?? {} });
    if (cmd === 'response_history_append') {
      if (appendFails) throw new Error('append failed');
      // 真实命令总会返回它存下的那一行。退回回显请求，是让忘了设
      // `appendResult` 的测试老老实实失败，而不是崩在 undefined 上。
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
    // 两者仍在本地累积——那是历史条渲染的东西。
    assert.equal(useAppStore.getState().responseHistory[REAL_ID]?.length, 1);
    assert.equal(useAppStore.getState().responseHistory[DRAFT_ID]?.length, 1);
  });

  it('落盘失败时保留乐观行，且不抛出', async () => {
    appendFails = true;
    const { addResponseHistory } = useAppStore.getState();

    assert.doesNotThrow(() => addResponseHistory(REAL_ID, makeResponse()));
    const before = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.equal(before.length, 1, 'the row is there before the IPC is even awaited');

    // 把 fire-and-forget 的 promise 排空。这里的未处理 rejection 会让整轮失败，
    // 而不是让断言失败。
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
    // 每次调用用不同的 id，且第二次追加**先** resolve——这正是任何把替换键设在比
    // 乐观行自身 id 更弱的依据上的实现会被打乱的形状。
    let n = 0;
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'response_history_append') {
        n += 1;
        const id = `rh-${n}`;
        await new Promise((resolve) => setTimeout(resolve, n === 2 ? 0 : 5));
        // 像真实命令那样回显 payload，好让串位的替换表现为状态不对，
        // 而不是两行一模一样。
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
    appendFails = true; // 让 store 侧走乐观路径
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

describe('loadResponseHistory 按接口加载', () => {
  it('只写自己那个接口的键，并把服务端的 newest-first 归一成 newest-last', async () => {
    // `list_by_endpoint` 回答的是 `created_at DESC, rowid DESC`。
    listResult = [
      makeEntry('epA', 'a3', { createdAt: 3 }),
      makeEntry('epA', 'a2', { createdAt: 2 }),
      makeEntry('epA', 'a1', { createdAt: 1 }),
    ];
    await useAppStore.getState().loadResponseHistory('epA');

    assert.deepEqual(calls[0].args, { endpointId: 'epA' }, '按接口查，不是按项目查');
    assert.deepEqual(Object.keys(useAppStore.getState().responseHistory), ['epA']);
    assert.deepEqual(
      useAppStore.getState().responseHistory.epA?.map((r) => r.id),
      ['a1', 'a2', 'a3'],
      'store 是 newest-last —— appendCapped 从尾部追加、从头部淘汰，' +
        '照抄服务端的 newest-first 会把这两头同时搞反',
    );
  });

  it('重开后下一次发送淘汰的仍是与 SQLite 同一条：最旧的', async () => {
    // 十行，就像数据库会回答的那样。
    listResult = Array.from({ length: 10 }, (_, i) =>
      makeEntry('epA', `rh-${i + 1}`, { createdAt: 1_000 + i }),
    ).reverse();

    await useAppStore.getState().loadResponseHistory('epA');
    useAppStore.getState().addResponseHistory('epA', makeResponse({ status: 201 }));

    const rows = useAppStore.getState().responseHistory.epA ?? [];
    assert.equal(rows.length, 10, '内存上限对重开的接口一样生效');
    assert.equal(
      rows[0].id,
      'rh-2',
      'rh-1 被淘汰 —— SQLite 裁掉的也是它；顺序反了的话这里丢的是 rh-10',
    );
    assert.equal(rows.at(-1)?.response.status, 201, '新发的那一条在最后');
  });

  it('首次打开：读取晚于发送落地时，乐观行不能被吃掉', async () => {
    let releaseList: () => void = () => undefined;
    let releaseAppend: () => void = () => undefined;
    const listGateP = new Promise<void>((resolve) => {
      releaseList = resolve;
    });
    const appendGateP = new Promise<void>((resolve) => {
      releaseAppend = resolve;
    });
    const stored = makeEntry(REAL_ID, 'rh-new', { createdAt: 2_000, bodyTruncated: true });

    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'response_history_list') {
        await listGateP;
        // 用户按下 Send 之前服务端拍下的快照。
        return [makeEntry(REAL_ID, 'rh-old', { createdAt: 1_000 })];
      }
      if (cmd === 'response_history_append') {
        await appendGateP;
        return stored;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });

    const loading = useAppStore.getState().loadResponseHistory(REAL_ID);
    useAppStore.getState().addResponseHistory(REAL_ID, makeResponse({ body: 'fresh' }));

    // 读取回来时乐观行还在途。
    releaseList();
    await loading;

    const mid = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.equal(mid.length, 2, '读取没有擦掉在途的乐观行');
    assert.equal(mid[0].id, 'rh-old');
    assert.ok(mid[1].id.startsWith('pending:'), '新发的那一条还在原地等着被服务端返回的行替换');

    releaseAppend();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const after = useAppStore.getState().responseHistory[REAL_ID] ?? [];
    assert.deepEqual(
      after.map((r) => r.id),
      ['rh-old', 'rh-new'],
      '落盘仍替换到了自己那条乐观行，而不是找不到就静默丢弃',
    );
    assert.equal(after[1].bodyTruncated, true, '裁剪信息只有服务端那一行有');
  });

  it('落盘后到达的读取不会把同一行算成两条', async () => {
    listResult = [
      makeEntry('epA', 'a2', { createdAt: 2 }),
      makeEntry('epA', 'a1', { createdAt: 1 }),
    ];
    await useAppStore.getState().loadResponseHistory('epA');
    // 重读同一份快照——应用里 `ResponseHistory` 会跳过这一次，
    // 但一行重复终归是错的答案。
    await useAppStore.getState().loadResponseHistory('epA');

    const rows = useAppStore.getState().responseHistory.epA ?? [];
    assert.equal(rows.length, 2, '按 id 去重，不是拼接');
    assert.deepEqual(
      rows.map((r) => r.id),
      ['a1', 'a2'],
    );
  });

  it('不再需要项目级竞态守卫：迟到的结果仍写回它自己的键', async () => {
    let release: () => void = () => undefined;
    listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    listResult = [makeEntry('epA', 'a1')];

    const loading = useAppStore.getState().loadResponseHistory('epA');
    // 读取在途时用户切到了另一个 endpoint。
    useAppStore.setState({ activeEndpointId: 'epB' });
    release();
    await loading;

    assert.deepEqual(
      Object.keys(useAppStore.getState().responseHistory),
      ['epA'],
      'a per-endpoint result is that endpoint 的 rows, so it is correct ' +
        'wherever it lands; dropping it would lose data the server already has',
    );
  });

  it('一个接口的加载不清掉另一个接口已经拿到的行', async () => {
    listResult = [makeEntry('epA', 'a1')];
    await useAppStore.getState().loadResponseHistory('epA');

    listResult = [];
    await useAppStore.getState().loadResponseHistory('epB');

    assert.deepEqual(
      Object.keys(useAppStore.getState().responseHistory).sort(),
      ['epA', 'epB'],
      "epB's empty history must not wipe epA's rows",
    );
    assert.equal(useAppStore.getState().responseHistory.epB?.length, 0);
  });

  it('加载失败不抛出，也不写 bootstrapError', async () => {
    listFails = true;
    await assert.doesNotReject(() => useAppStore.getState().loadResponseHistory('epA'));
    assert.equal(useAppStore.getState().bootstrapError, null);
    assert.deepEqual(useAppStore.getState().responseHistory, {});
  });

  it('draft 接口不发请求', async () => {
    await useAppStore.getState().loadResponseHistory(DRAFT_ID);
    assert.deepEqual(
      cmdNames(),
      [],
      'a draft endpoint has no nodes row, so the server has nothing to read',
    );
  });

  it('空 id 不发请求', async () => {
    await useAppStore.getState().loadResponseHistory('');
    assert.deepEqual(cmdNames(), []);
  });
});
