/**
 * `src/stores/crud-slice.ts` 的切片级测试。
 *
 * 覆盖：
 *   - 每个 CRUD 动作都是同一种形状——用 `set()` 乐观改本地状态、fire-and-forget
 *     的 `invoke<...>(COMMANDS.*)`、出错时用 `set()` 回滚到先前状态。这里挑
 *     `updateEndpointDetail` 一族，因为它是树里被调用最多的乐观动作，且正好跑
 *     过乐观 + 回滚这条分支。
 *   - 在 store 里种下某个 endpoint 的 detail 行。
 *   - 对着一个会 reject 的 `invoke` mock 调 `updateEndpointDetail`。
 *   - await 返回的 promise（该动作会 await 这次 IPC）。
 *   - 断言回滚恢复了原来的 detail——同一个对象引用——并且有一条告警被记下。
 *
 * 运行：
 *   npx tsx --test src/stores/__tests__/crud-slice.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { createCrudSlice } from '../crud-slice';
import { resolveEndpointBaseUrl } from '@/lib/url';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';

/** 一个最小但有领域形状的 `EndpointDetail`。 */
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
 * 一个拥有三个服务的项目。不再有任何按 module 划分的范围——每个 module 都看到
 * 全部三个——所以下面的测试断言的是那个星形结构是一个项目级 id，而不是每个 module
 * 一个标志位。
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
  // 在 console.warn / console.error 上装探针，好让我们能断言回滚分支
  // 记下了 IPC 失败（生产行为）。
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
// T-2 —— 乐观更新 + 回滚
// ============================================================

describe('crud-slice: updateEndpointDetail — T-2 optimistic + rollback', () => {
  it('applies optimistic update, then rolls back when invoke rejects', async () => {
    const original = makeDetail({ nodeId: 'ep-rollback', url: '/widgets' });
    const harness = makeCrudHarness({ 'ep-rollback': original });

    // 让 `endpoint_detail_update` reject。切片的 catch 分支应当触发回滚的
    // set()，并用 `console.error` 记下这次失败。
    installIpcMock(async (cmd) => {
      if (cmd === 'endpoint_detail_update') {
        throw new Error('sqlite: disk I/O error');
      }
      throw new Error('unexpected cmd: ' + cmd);
    });

    // 给下面的相等性断言拍下调用前的状态快照。
    const beforeUpdate = harness.bag.endpointDetails['ep-rollback'];
    assert.equal(beforeUpdate.url, '/widgets');

    const action = harness.slice.updateEndpointDetail('ep-rollback', {
      method: 'POST',
      url: '/widgets/v2',
    });

    // IPC 落定之前乐观写入就已可见。
    assert.equal(harness.bag.endpointDetails['ep-rollback'].method, 'POST');
    assert.equal(harness.bag.endpointDetails['ep-rollback'].url, '/widgets/v2');

    await action;

    const after = harness.bag.endpointDetails['ep-rollback'];
    // 原来的 detail 必须仍原封不动地在 store 里。
    assert.ok(after, 'detail not deleted');
    assert.equal(after.nodeId, 'ep-rollback', 'same id');
    assert.equal(after.method, 'GET', 'optimistic method change rolled back');
    assert.equal(after.url, '/widgets', 'optimistic url change rolled back');

    // 切片应当发出带同一条错误消息的 console 告警/错误——生产环境正是这样把
    // 失败暴露给用户的。两条通道都接受，因为切片两条都用（poll 那种用
    // `console.warn`，写入用 `console.error`）。
    const seenFailure = warnings.some(
      (w) => /sqlite.*disk I.O. error/i.test(w) || /Error: sqlite: disk I\/O error/i.test(w),
    );
    assert.ok(
      seenFailure,
      'failure should have been logged to console.warn|console.error; got: ' +
        JSON.stringify(warnings),
    );

    // 核对：恢复出来的对象引用的正是切片持有的那个 `prev` 捕获值。断言
    // 引用相等反而太强——切片先把乐观对象存进映射，回滚时再用 `prev` 替换它，
    // 所以我们看到的**就是** `prev`。
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
    // 从两次 `environments_set_var`（先 null 再赋值）改成一次调用，是因为前者是两个
    // 事务、中间有一个变量不存在的窗口。现在改名是一次原子的后端调用。
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    const echoed = makeEnvironment({ variables: { renamed: 'value', keep: 'yes' } });
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return echoed;
    });

    const action = harness.slice.renameEnvironmentVar('env-1', 'old', 'renamed');

    // 乐观：交换立刻可见，中间没有变量缺失的状态。
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
// addNode —— detail 映射必须按服务端返回的 id 作键
// ============================================================

describe('crud-slice: addNode 的 id 以服务端返回值为准', () => {
  /** 一个后端会交还的节点，带着它自己的 id。 */
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
    // Tauri 的 `nodes_create` 命令自己铸一个 `uid()` 并丢掉调用方发的 id，
    // 所以这就是桌面端的形状。
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
    // 这个守卫防的 bug：按本地 id 作键会把 detail 写在一个没有任何节点持有的键之下，
    // 于是 `EndpointList` 跳过这一行，工作区把标签解析成它的"已删除"描述符。
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
    // REST 的形状：`ensure_server_id` 保留调用方提供的 id。
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

// ============================================================
// addBaseUrlDefinition —— 渲染侧铸 id，后端采纳它
//
// 要防的 bug 是"新建的服务解析不出 URL"那个。有两处在这一行落盘之前就按服务
// id 作键——项目的 ★ 和每个 `env.moduleBaseUrls[defId][moduleId]` 单元格——
// 后端铸的 id 会让两者都指着一行从未写入的行，`resolveEndpointBaseUrl` 于是对
// 用户刚配好的服务回答 ''。所以断言终止在解析器而不是请求形状上：一个熬过往返
// 之后仍然能解析的 id，就是全部的契约。
// ============================================================

describe('crud-slice: 新建 baseUrl 服务的 id 前端铸、后端采纳', () => {
  /** 一个还没有任何服务的项目，★ 也是空的。 */
  function emptyProject(): Project {
    return makeProject({ baseUrlDefinitions: [], defaultBaseUrlDefinitionId: null });
  }

  it('本地 id 随请求下发，服务行与 ★ 留在同一个 id 上', async () => {
    const harness = makeCrudHarness({}, [makeEnvironment()], [], [emptyProject()]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'base_url_defs_create') {
        const opts = (args?.opts ?? {}) as { id?: string };
        // 修复后的契约：后端采纳调用方的 id。
        return { id: opts.id, name: String(args?.name) };
      }
      throw new Error('unexpected cmd: ' + cmd);
    });

    const defId = harness.slice.addBaseUrlDefinition('project-1', 'auth');
    assert.ok(defId, 'addBaseUrlDefinition 必须同步返回 id，设置表要拿它种单元格');

    const create = calls.find((c) => c.cmd === 'base_url_defs_create');
    assert.ok(create, '必须发出 base_url_defs_create');
    assert.equal(
      (create!.args.opts as { id?: string }).id,
      defId,
      '请求必须带上本地 id，否则 ★ 和单元格会和落库的行脱钩',
    );

    // 让 create 的 .then 落定（它把返回的行换进列表）。
    await new Promise((r) => setTimeout(r, 0));

    const project = harness.bag.projects[0];
    assert.deepEqual(
      project.baseUrlDefinitions.map((d) => d.id),
      [defId],
      '服务行的 id 必须还是那个本地 id',
    );
    assert.ok(
      project.baseUrlDefinitions.some((d) => d.id === project.defaultBaseUrlDefinitionId),
      `★ (${project.defaultBaseUrlDefinitionId}) 必须指向列表里真实存在的一行，` +
        '否则项目默认永远解析为空',
    );
  });

  it('按返回的 id 填完单元格后，解析器读得到地址（显式选择与项目默认两条路）', async () => {
    const harness = makeCrudHarness({}, [makeEnvironment()], [], [emptyProject()]);
    installIpcMock(async (cmd, args) => {
      if (cmd === 'base_url_defs_create') {
        const opts = (args?.opts ?? {}) as { id?: string };
        return { id: opts.id, name: String(args?.name) };
      }
      if (cmd === 'environments_set_module_baseurl') return null;
      throw new Error('unexpected cmd: ' + cmd);
    });

    const defId = harness.slice.addBaseUrlDefinition('project-1', 'auth');
    await harness.slice.setEnvironmentModuleBaseUrl(
      'env-1',
      defId,
      'mod_a',
      'https://api.demo.dev',
    );
    await new Promise((r) => setTimeout(r, 0));

    const project = harness.bag.projects[0];
    const env = harness.bag.environments[0];
    const none = { environmentId: null, baseUrlDefinitionId: null, baseUrlOverride: null };

    assert.equal(
      resolveEndpointBaseUrl({ ...none, baseUrlDefinitionId: defId }, env, project, 'mod_a')
        .baseUrl,
      'https://api.demo.dev',
      '显式选择该服务后必须解析出它的前置 URL',
    );
    assert.equal(
      resolveEndpointBaseUrl(none, env, project, 'mod_a').baseUrl,
      'https://api.demo.dev',
      '项目默认（★）也必须解析出同一个地址',
    );
  });

  it('后端铸了另一个 id 时，本地行回滚而不是留下一条悬空 ★', async () => {
    const harness = makeCrudHarness({}, [makeEnvironment()], [], [emptyProject()]);
    installIpcMock(async (cmd) => {
      if (cmd === 'base_url_defs_create') throw new Error('UNIQUE constraint failed');
      throw new Error('unexpected cmd: ' + cmd);
    });

    harness.slice.addBaseUrlDefinition('project-1', 'auth');
    await new Promise((r) => setTimeout(r, 0));

    const project = harness.bag.projects[0];
    assert.deepEqual(project.baseUrlDefinitions, [], '写失败时乐观行要被撤掉');
    assert.equal(project.defaultBaseUrlDefinitionId, null, '★ 不能停在一个不存在的 id 上');
  });
});
