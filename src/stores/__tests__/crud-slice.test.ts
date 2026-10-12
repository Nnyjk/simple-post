/**
 * Slice-level test for `src/stores/crud-slice.ts`.
 *
 * Target from the second-pass audit (P2 tier):
 *   - T-2: Every CRUD slice action follows the same shape —
 *     optimistic local mutation via `set()`, fire-and-forget
 *     `invoke<...>(COMMANDS.*)`, on error rollback to the previous
 *     state via `set()`. We pick the endpoint-detail family
 *     (`updateEndpointDetail`) because it's the most-called optimistic
 *     action in the tree and it exercises the optimistic+rollback branch.
 *
 * Coverage:
 *   - Seeds an endpoint's detail row in the store.
 *   - Invokes `updateEndpointDetail` against an `invoke` mock that
 *     rejects.
 *   - Awaits the returned promise (the action awaits the IPC).
 *   - Asserts the rollback restored the original detail — same object
 *     reference — and that a warning was logged.
 *
 * Run via:
 *   npx tsx --test src/stores/__tests__/crud-slice.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { createCrudSlice } from '../crud-slice';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';

/** A minimal but domain-shaped `EndpointDetail`. */
function makeDetail(overrides: Partial<EndpointDetail> = {}): EndpointDetail {
  return {
    nodeId: 'ep-1',
    method: 'GET',
    url: '/widgets',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags: [],
    ...overrides,
  };
}

/** 用于环境值增删 action 的最小环境数据。 */
function makeEnvironment(overrides: Partial<Environment> = {}): Environment {
  return {
    id: 'env-1',
    projectId: 'project-1',
    name: 'dev',
    baseUrls: {},
    moduleBaseUrls: {},
    variables: {},
    isActive: true,
    ...overrides,
  };
}

/**
 * A project owning three services. There is no per-module scoping any
 * more — every module sees all three — so the tests below assert the star
 * is a single project-wide id rather than a flag per module.
 */
function makeProject(overrides: Partial<Project> = {}): Project {
  return {
    id: 'project-1',
    name: '项目',
    baseUrlDefinitions: [
      { id: 'd-1', name: '默认服务' },
      { id: 'd-2', name: '数据服务' },
      { id: 'd-3', name: '基础服务' },
    ],
    defaultBaseUrlDefinitionId: 'd-1',
    color: '#000000',
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

/**
 * 构造 CRUD slice 所需的最小 store；环境 action 读取 environments，
 * endpoint action 读取 endpointDetails，baseUrl action 读取 projects，
 * 二者共用 Zustand 风格的 setter。
 */
type CrudHarnessState = {
  nodes: ResourceNode[];
  endpointDetails: Record<string, EndpointDetail>;
  environments: Environment[];
  projects: Project[];
  openTabs: never[];
  activeTabId: null;
  activeEndpointId: null;
};

function makeCrudHarness(
  initialDetails: Record<string, EndpointDetail> = {},
  initialEnvironments: Environment[] = [],
  initialNodes: ResourceNode[] = [],
  initialProjects: Project[] = [],
) {
  const bag: CrudHarnessState = {
    nodes: initialNodes,
    endpointDetails: initialDetails,
    environments: initialEnvironments,
    projects: initialProjects,
    openTabs: [],
    activeTabId: null,
    activeEndpointId: null,
  };
  const set = (partial: unknown) => {
    const updater =
      typeof partial === 'function'
        ? (partial as (state: CrudHarnessState) => unknown)(bag)
        : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = createCrudSlice(set as never, get as never, {} as never);
  return { bag, slice };
}

let warnings: string[] = [];
let origWarn: typeof console.warn;
let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  // Spy on console.warn / console.error so we can assert the
  // rollback branch logged the IPC failure (production behaviour).
  origWarn = console.warn.bind(console);
  origError = console.error.bind(console);
  warnings = [];
  const fmt = (a: unknown) => {
    if (typeof a === 'string') return a;
    if (a instanceof Error) return `${a.name}: ${a.message}`;
    try {
      return JSON.stringify(a);
    } catch {
      return String(a);
    }
  };
  console.warn = (...args: unknown[]) => {
    warnings.push(args.map(fmt).join(' '));
  };
  console.error = (...args: unknown[]) => {
    warnings.push(args.map(fmt).join(' '));
  };
});

afterEach(() => {
  console.warn = origWarn;
  console.error = origError;
  clearIpcMock();
});

// ============================================================
// T-2 — optimistic update + rollback
// ============================================================

describe('crud-slice: updateEndpointDetail — T-2 optimistic + rollback', () => {
  it('applies optimistic update, then rolls back when invoke rejects', async () => {
    const original = makeDetail({ nodeId: 'ep-rollback', url: '/widgets' });
    const harness = makeCrudHarness({ 'ep-rollback': original });

    // Mock `endpoint_detail_update` to reject. The slice's catch branch
    // should fire the rollback set() and `console.error` the failure.
    installIpcMock(async (cmd) => {
      if (cmd === 'endpoint_detail_update') {
        throw new Error('sqlite: disk I/O error');
      }
      throw new Error('unexpected cmd: ' + cmd);
    });

    // Snapshot pre-call state for the equality assertion below.
    const beforeUpdate = harness.bag.endpointDetails['ep-rollback'];
    assert.equal(beforeUpdate.url, '/widgets');

    const action = harness.slice.updateEndpointDetail('ep-rollback', {
      method: 'POST',
      url: '/widgets/v2',
    });

    // Optimistic write is visible before the IPC settles.
    assert.equal(harness.bag.endpointDetails['ep-rollback'].method, 'POST');
    assert.equal(harness.bag.endpointDetails['ep-rollback'].url, '/widgets/v2');

    await action;

    const after = harness.bag.endpointDetails['ep-rollback'];
    // The original detail must still be in the store untouched.
    assert.ok(after, 'detail not deleted');
    assert.equal(after.nodeId, 'ep-rollback', 'same id');
    assert.equal(after.method, 'GET', 'optimistic method change rolled back');
    assert.equal(after.url, '/widgets', 'optimistic url change rolled back');

    // The slice should have emitted a console warning/error carrying
    // the same error message — that's how the prod UX surfaces the
    // failure. We accept either channel since the slice uses both
    // `console.warn` (for 'poll' style) and `console.error` (for
    // writes) — see CRUD slice header.
    const seenFailure = warnings.some(
      (w) => /sqlite.*disk I.O. error/i.test(w) || /Error: sqlite: disk I\/O error/i.test(w),
    );
    assert.ok(
      seenFailure,
      'failure should have been logged to console.warn|console.error; got: ' +
        JSON.stringify(warnings),
    );

    // Sanity: the recovered object references the very same `prev`
    // capture the slice held (or an object equal to it). Identity
    // would be too strong because the slice stores the optimistic
    // object back into the map, then replaces it with `prev` on
    // rollback — so what we should see IS `prev`.
    assert.strictEqual(
      after,
      beforeUpdate,
      'rolled-back detail reuses the prev snapshot reference',
    );
  });

  it('未知 nodeId 不发 IPC — 没有 detail 行可乐观写入', async () => {
    const harness = makeCrudHarness({});
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.updateEndpointDetail('ghost', { url: '/nope' });

    assert.deepEqual(calls, [], '没有 detail 行就不该打扰后端');
  });
});

type CapturedInvoke = { cmd: string; args: Record<string, unknown> };

describe('crud-slice: 环境值的 null 删除契约', () => {
  it('变量删除立即移除本地键，发送 null，并在成功路径保留空字符串值', async () => {
    const original = makeEnvironment({
      variables: { token: 'secret', empty: '' },
    });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });

    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      await pending;
      return null;
    });

    const action = harness.slice.setEnvironmentVar('env-1', 'token', null);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      { empty: '' },
      'null 应在 IPC 完成前从乐观状态移除变量',
    );

    release();
    await action;
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'token', value: null },
      },
    ]);

    await harness.slice.setEnvironmentVar('env-1', '', '');
    assert.deepEqual(
      harness.bag.environments[0].variables,
      { empty: '', '': '' },
      '空字符串是真实变量值，不应被当作删除',
    );
  });

  it('变量删除失败时恢复原变量和值', async () => {
    const original = makeEnvironment({ variables: { token: 'secret', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => {
      throw new Error('variable delete failed');
    });

    await harness.slice.setEnvironmentVar('env-1', 'token', null);

    assert.deepEqual(harness.bag.environments[0].variables, original.variables);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('模块 BaseUrl null 删除单元格并发送 null，空 URL 仍作为真实值保留', async () => {
    const original = makeEnvironment({
      moduleBaseUrls: {
        api: { 'mod-1': 'https://api.example.test', 'mod-2': 'https://api2.example.test' },
        empty: { 'mod-1': '' },
      },
    });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    // 删掉 mod-1 的格子，mod-2 的同服务格子必须原样保留 ——
    // 每个 (模块 × 服务) 都是独立值，没有共享。
    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'api', 'mod-1', null);
    assert.equal(
      Object.hasOwn(harness.bag.environments[0].moduleBaseUrls.api ?? {}, 'mod-1'),
      false,
      'null 应从乐观状态移除该模块的单元格',
    );
    assert.equal(
      harness.bag.environments[0].moduleBaseUrls.api?.['mod-2'],
      'https://api2.example.test',
      '清除一个模块的格子不能动到同一服务的其它模块格子',
    );
    assert.equal(harness.bag.environments[0].moduleBaseUrls.empty?.['mod-1'], '');

    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'empty', 'mod-1', '');
    assert.equal(harness.bag.environments[0].moduleBaseUrls.empty?.['mod-1'], '');
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_module_baseurl',
        args: { envId: 'env-1', defId: 'api', moduleId: 'mod-1', url: null },
      },
      {
        cmd: 'environments_set_module_baseurl',
        args: { envId: 'env-1', defId: 'empty', moduleId: 'mod-1', url: '' },
      },
    ]);
  });

  it('模块 BaseUrl 删除失败时恢复原单元格和值', async () => {
    const original = makeEnvironment({
      moduleBaseUrls: { api: { 'mod-1': 'https://api.example.test' } },
    });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => {
      throw new Error('base URL delete failed');
    });

    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'api', 'mod-1', null);

    assert.deepEqual(harness.bag.environments[0].moduleBaseUrls, original.moduleBaseUrls);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('项目默认服务：写单个 id，null 清除默认', async () => {
    const original = makeProject();
    const harness = makeCrudHarness({}, [], [], [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.setDefaultBaseUrlDefinition('project-1', 'd-2');

    // ★ 是项目上的一个 id，不是每行上的一个标志位，所以不存在
    // 「同时有两个 ★」的中间态。
    assert.equal(harness.bag.projects[0].defaultBaseUrlDefinitionId, 'd-2');
    // 服务列表本身一个字节都不该动。
    assert.strictEqual(harness.bag.projects[0].baseUrlDefinitions, original.baseUrlDefinitions);

    await harness.slice.setDefaultBaseUrlDefinition('project-1', null);
    assert.equal(
      harness.bag.projects[0].defaultBaseUrlDefinitionId,
      null,
      'null 表示项目刻意没有默认服务，而不是"恢复某个模块的 ★"',
    );

    assert.deepEqual(calls, [
      { cmd: 'base_url_defs_set_default', args: { projectId: 'project-1', defId: 'd-2' } },
      { cmd: 'base_url_defs_set_default', args: { projectId: 'project-1', defId: null } },
    ]);
  });

  it('项目默认服务写入失败时整体回滚', async () => {
    const original = makeProject();
    const harness = makeCrudHarness({}, [], [], [original]);
    installIpcMock(async () => {
      throw new Error('set default failed');
    });

    await harness.slice.setDefaultBaseUrlDefinition('project-1', 'd-2');

    assert.equal(harness.bag.projects[0].defaultBaseUrlDefinitionId, 'd-1');
    assert.strictEqual(harness.bag.projects[0], original);
  });

  it('重命名只发一次 environments_rename_var，不再拆成删除+写入两次调用', async () => {
    // The previous shape sent `environments_set_var(old, null)` followed
    // by `environments_set_var(new, value)` — two transactions with a
    // window in between where the variable did not exist. The rename is
    // now one atomic backend call.
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    const echoed = makeEnvironment({ variables: { renamed: 'value', keep: 'yes' } });
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return echoed;
    });

    const action = harness.slice.renameEnvironmentVar('env-1', 'old', 'renamed');

    // Optimistic: the swap is visible immediately, with no intermediate
    // state in which the variable is missing.
    assert.deepEqual(harness.bag.environments[0].variables, {
      renamed: 'value',
      keep: 'yes',
    });

    await action;

    assert.deepEqual(calls, [
      {
        cmd: 'environments_rename_var',
        args: { envId: 'env-1', fromKey: 'old', toKey: 'renamed' },
      },
    ]);
    assert.deepEqual(harness.bag.environments[0].variables, {
      renamed: 'value',
      keep: 'yes',
    });
  });

  it('重命名失败时恢复完整快照 — 后端从未进入过删除状态', async () => {
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      throw new Error('rename failed');
    });

    await harness.slice.renameEnvironmentVar('env-1', 'old', 'renamed');

    assert.deepEqual(calls, [
      {
        cmd: 'environments_rename_var',
        args: { envId: 'env-1', fromKey: 'old', toKey: 'renamed' },
      },
    ]);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      original.variables,
      '单次事务失败时变量必须完好，不存在“已删除未重建”的中间态',
    );
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('重命名的 key 不存在时不发 IPC', async () => {
    const original = makeEnvironment({ variables: { keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.renameEnvironmentVar('env-1', 'ghost', 'renamed');

    assert.deepEqual(calls, [], '源 key 不存在时不应打扰后端');
    assert.deepEqual(harness.bag.environments[0].variables, { keep: 'yes' });
  });
});

// ============================================================
// addNode — the detail map must key off the id the server returned
// ============================================================

describe('crud-slice: addNode 的 id 以服务端返回值为准', () => {
  /** A node as the backend would hand it back, with its own id. */
  function serverNode(id: string, overrides: Partial<ResourceNode> = {}): ResourceNode {
    return {
      id,
      rootId: 'p1',
      parentId: 'm1',
      kind: 'endpoint',
      name: 'Login',
      sortOrder: 0,
      createdAt: 0,
      updatedAt: 0,
      ...overrides,
    };
  }

  it('后端另起 id 时，detail 挂在它返回的 id 上而不是本地 id', async () => {
    const harness = makeCrudHarness();
    // The Tauri `nodes_create` command mints its own `uid()` and drops
    // the id the caller sent, so this is the desktop shape.
    installIpcMock(async (cmd) => {
      if (cmd === 'nodes_create') return serverNode('server-id');
      throw new Error('unexpected cmd: ' + cmd);
    });

    const returned = await harness.slice.addNode({
      parentId: 'm1',
      kind: 'endpoint',
      name: 'Login',
      detail: { url: '' },
    });

    assert.equal(returned, 'server-id', '返回的 id 必须是服务端那个');
    assert.equal(harness.bag.nodes.at(-1)?.id, 'server-id');
    // The bug this guards: keying by the local id wrote the detail
    // under a key no node carries, so `EndpointList` skipped the row
    // and the workspace resolved the tab to its "已删除" descriptor.
    assert.ok(
      harness.bag.endpointDetails['server-id'],
      'detail 必须按服务端 id 落位，否则树里那一行永远渲染不出来',
    );
    assert.equal(
      Object.keys(harness.bag.endpointDetails).length,
      1,
      '不应在本地 id 下留下一条孤儿 detail',
    );
    assert.equal(harness.bag.endpointDetails['server-id'].nodeId, 'server-id');
  });

  it('后端沿用调用方 id 时行为一致', async () => {
    const harness = makeCrudHarness();
    // The REST shape: `ensure_server_id` keeps a caller-supplied id.
    installIpcMock(async (cmd, args) => {
      if (cmd === 'nodes_create') return serverNode(String(args?.id));
      throw new Error('unexpected cmd: ' + cmd);
    });

    const returned = await harness.slice.addNode({
      parentId: 'm1',
      kind: 'endpoint',
      name: 'Login',
      detail: { url: '' },
    });

    assert.equal(returned, harness.bag.nodes.at(-1)?.id);
    assert.ok(harness.bag.endpointDetails[returned!], 'detail 必须挂在返回的 id 上');
    assert.equal(Object.keys(harness.bag.endpointDetails).length, 1);
  });

  it('非 endpoint 不写 detail 行', async () => {
    const harness = makeCrudHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'nodes_create') return serverNode('m-server', { kind: 'module', parentId: null });
      throw new Error('unexpected cmd: ' + cmd);
    });

    await harness.slice.addNode({ rootId: 'p1', kind: 'module', name: 'Core' });

    assert.deepEqual(
      harness.bag.endpointDetails,
      {},
      '给模块写一条空 detail 会让 Object.keys(endpointDetails) 说谎',
    );
  });
});
