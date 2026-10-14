/**
 * `src/lib/tauri-commands.ts`（双形态 `invokeCmd` 包装）的单测。
 *
 * Tauri 桌面 shell 与浏览器 / serve-web 形态共用同一个渲染层调用点
 * （`invokeCmd`）。这个包装需要做到：
 *
 *   1. 在模块加载时经 `window.__TAURI_INTERNALS__` / `window.__TAURI__` 判定形态。
 *   2. 把 `COMMANDS` 表里每个 invoke 命令名映射成 web 形态的
 *      `(method, path, body)` 三元组——包括路径参数编码、列表过滤的查询串投影，
 *      以及路径参数必须从线上载荷里剥离时的 body 投影（如 `projects_update`）。
 *   3. 命令在 `WEB_ROUTES` 里缺失时抛一个自带说明的错误，把开发者指向该加它的文件。
 *
 * 本文件覆盖什么
 * --------------
 *   * `IS_TAURI_SHELL` 通过子 Node 进程验证，好让模块加载时的求值在隔离中跑
 *     （那个 const 在同一进程里没法重新切换——见 `tauri-commands.ts` 顶部的表）。
 *   * web 形态的 `invokeCmd` 通过桩掉全局 `fetch` 来跑。我们断言
 *     `(method, url, 解析后的 body)`，这样路由路径的静默漂移会响亮地失败。
 *   * 路径参数编码（`encodeURIComponent`）在一批有代表性的入口上检查。
 *   * 路径参数与 body 的分离（`body:` 投影器）在 `projects_update` 上检查
 *     （另有若干入口靠副作用顺带覆盖）。
 *   * 未知命令的抛错同时把开发者指向 `tauri-commands.ts` 与 `web_api/`。
 *
 * 运行：
 *   npx tsx --test src/lib/tauri-commands.test.ts
 */

import { describe, it, before, after, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';

import { COMMANDS, IS_TAURI_SHELL, TauriCommand, invokeCmd, readApiBase } from '../tauri-commands';

// ---------------------------------------------------------------------------
// Fetch mock plumbing.
// ---------------------------------------------------------------------------

interface CapturedCall {
  url: string;
  /** String for easier assertions; `undefined` when the request had no body. */
  body: string | undefined;
  method: string;
  contentType: string | undefined;
  signal: AbortSignal | undefined;
}

let calls: CapturedCall[] = [];
const originalFetch = globalThis.fetch;

function installFetchMock(respondWith: { json?: unknown; status?: number } = {}): void {
  calls = [];
  const status = respondWith.status ?? 200;
  const json = respondWith.json;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = typeof input === 'string' ? input : input.toString();
    let bodyStr: string | undefined;
    if (typeof init?.body === 'string') bodyStr = init.body;
    const contentType = init?.headers
      ? (new Headers(init.headers).get('content-type') ?? undefined)
      : undefined;
    calls.push({
      url,
      body: bodyStr,
      method: init?.method ?? 'GET',
      contentType,
      signal: init?.signal ?? undefined,
    });
    const text = json === undefined ? '' : JSON.stringify(json);
    return new Response(text, {
      status,
      headers: { 'content-type': 'application/json' },
    });
  }) as typeof fetch;
}

function restoreFetchMock(): void {
  globalThis.fetch = originalFetch;
  calls = [];
}

// ---------------------------------------------------------------------------
// IS_TAURI_SHELL —— 必须放在隔离的子进程里跑，因为该值在模块求值时就被捕获了。
// ---------------------------------------------------------------------------

/**
 * 起一个全新的 Node 进程，加载 `tauri-commands.ts` 并把 `IS_TAURI_SHELL` 以 JSON
 * 打印出来。可选的 `window` 对象会在 import 模块**之前**装到 `globalThis` 上，好让
 * 模块加载时的检测看到正确的形态。
 */
function readIsTauriShellInChild(windowShim?: Record<string, unknown>): {
  value: boolean;
  stderr: string;
} {
  const setup = windowShim
    ? `globalThis.window = ${JSON.stringify(windowShim)};`
    : `delete globalThis.window;`;
  // 子进程里的动态 import 在 Windows 上需要 `file://` URL；裸的绝对路径会被当成
  // URL 解析并拒绝。`new URL(rel, import.meta.url).href` 返回的就是 file URL 字符串，
  // 正是动态 `import()` 要的形状。相对*本*文件，所以要 `../` 跳出 `__tests__`。
  const moduleUrl = new URL('../tauri-commands.ts', import.meta.url).href;
  const code = [
    setup,
    `import('${moduleUrl}')`,
    `.then((m) => {`,
    `  process.stdout.write(JSON.stringify(m.IS_TAURI_SHELL));`,
    `})`,
    `.catch((e) => {`,
    `  process.stderr.write(String(e && e.stack || e));`,
    `  process.exit(1);`,
    `});`,
  ].join('\n');
  const result = spawnSync(
    process.execPath,
    ['--import', 'tsx/esm', '--input-type=module', '--eval', code],
    { encoding: 'utf-8' },
  );
  if (result.status !== 0) {
    throw new Error(`child failed (status=${result.status}): ${result.stderr}`);
  }
  return { value: JSON.parse(result.stdout.trim()), stderr: result.stderr };
}

describe('IS_TAURI_SHELL (child-process probe)', () => {
  it('is false when no window globals are set', () => {
    const { value } = readIsTauriShellInChild(undefined);
    assert.equal(value, false);
  });

  it('is true when window.__TAURI_INTERNALS__ is set', () => {
    const { value } = readIsTauriShellInChild({ __TAURI_INTERNALS__: {} });
    assert.equal(value, true);
  });

  it('is true when window.__TAURI__ (v1 shim) is set', () => {
    const { value } = readIsTauriShellInChild({ __TAURI__: {} });
    assert.equal(value, true);
  });

  it('in this test file (no window) it must be false', () => {
    // 健全性检查——保证本套件余下部分都跑在 web 形态。
    assert.equal(IS_TAURI_SHELL, false);
  });
});

// ---------------------------------------------------------------------------
// web 形态下的 invokeCmd —— 对 path / method / body 的断言。
// ---------------------------------------------------------------------------

describe('invokeCmd (web shape) — path & body mapping', () => {
  beforeEach(() => {
    installFetchMock({ json: { ok: true }, status: 200 });
  });

  after(() => {
    restoreFetchMock();
  });

  function last(): CapturedCall {
    assert.ok(calls.length > 0, 'fetch was not called');
    return calls[calls.length - 1] as CapturedCall;
  }

  it('projectsList → GET /api/projects', async () => {
    await invokeCmd(COMMANDS.projectsList);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/projects');
    assert.equal(c.body, undefined);
  });

  it('projectsGet → GET /api/projects/{id} with url-encoded id', async () => {
    await invokeCmd(COMMANDS.projectsGet, { id: 'a/b c' });
    const c = last();
    assert.equal(c.method, 'GET');
    // '/' 与 ' ' 必须百分号编码；不该有任何裸字符漏出去。
    assert.equal(c.url, '/api/projects/a%2Fb%20c');
  });

  it('projectsUpdate → PATCH /api/projects/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.projectsUpdate, {
      id: 'p1',
      patch: { name: 'New name' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/projects/p1');
    // `id` 绝不能漏进 body —— Rust handler 把 body 反序列化成 `ProjectPatch`，多出来的
    // `id` 字段轻则被静默忽略。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'New name' });
  });

  it('projectsDelete → DELETE /api/projects/{id}, no body', async () => {
    await invokeCmd(COMMANDS.projectsDelete, { id: 'p1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/projects/p1');
    assert.equal(c.body, undefined);
  });

  it('nodesList → GET /api/nodes with rootId in query string', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/nodes?rootId=pX');
    assert.equal(c.body, undefined);
  });

  it('nodesList (no filter) → GET /api/nodes (no query string)', async () => {
    // 列表命令在渲染层有时不带参数调用（`invoke<ResourceNode[]>(COMMANDS.nodesList)`）；
    // URL 不该因此多出一个尾随的 `?`。
    await invokeCmd(COMMANDS.nodesList);
    const c = last();
    assert.equal(c.url, '/api/nodes');
  });

  it('nodesList → projects type and query onto the same canonical route', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX', type: 'endpoint', query: 'login' });
    const c = last();
    // `buildQuery` 会给键排序，所以线上顺序是字母序（`q` < `rootId` < `type`），不是
    // 调用方的参数顺序。
    assert.equal(c.url, '/api/nodes?q=login&rootId=pX&type=endpoint');
  });

  it('nodesList → 三个过滤条件落到同一条路由，query 也会被编码', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX', type: 'endpoint', query: 'a b&c' });
    const c = last();
    assert.equal(c.url, '/api/nodes?q=a%20b%26c&rootId=pX&type=endpoint');
  });

  it('nodesGet → GET /api/nodes/{id} with the id percent-encoded', async () => {
    await invokeCmd(COMMANDS.nodesGet, { id: 'a/b' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/nodes/a%2Fb');
    assert.equal(c.body, undefined);
  });

  it('nodesCreate → POST /api/nodes with a FLAT body (NodeCreateBody flattens the node)', async () => {
    await invokeCmd(COMMANDS.nodesCreate, {
      id: 'n1',
      rootId: 'p1',
      parentId: 'c1',
      kind: 'endpoint',
      name: 'Login',
      description: 'auth',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes');
    // 节点自身的字段**就是** body。套一层 `{ node: {…} }` 会无法反序列进
    // `NodeCreateBody` 的 `#[serde(flatten)]`。`sortOrder` 在服务端没有默认值，所以
    // 缺省时必须变成 `0` 而不是 `undefined`（后者会被 `JSON.stringify` 丢掉）。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      id: 'n1',
      rootId: 'p1',
      parentId: 'c1',
      kind: 'endpoint',
      name: 'Login',
      description: 'auth',
      sortOrder: 0,
    });
  });

  it('nodesCreate → defaults parentId to null and stamps detail.nodeId from the id', async () => {
    await invokeCmd(COMMANDS.nodesCreate, {
      id: 'n1',
      rootId: 'p1',
      kind: 'module',
      name: 'Core',
      sortOrder: 3,
      detail: { method: 'POST', url: '/login' },
    });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    // module 的父级是项目，用 `null` 表达。
    assert.equal(body.parentId, null);
    assert.equal(body.sortOrder, 3);
    // 线上 `endpoint_details.node_id` 是必填；客户端本来就知道它，所以由投影注入，
    // 而不是让调用方重复一遍 id。
    assert.deepEqual(body.detail, { method: 'POST', url: '/login', nodeId: 'n1' });
  });

  it('nodesUpdate → PATCH /api/nodes/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.nodesUpdate, { id: 'n1', patch: { name: 'Renamed' } });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/nodes/n1');
    // `id` 绝不能漏进 body —— Rust handler 把 body 反序列化成 `NodePatch`。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('nodesDelete → DELETE /api/nodes/{id}, no body', async () => {
    await invokeCmd(COMMANDS.nodesDelete, { id: 'n1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/nodes/n1');
    assert.equal(c.body, undefined);
  });

  it('nodesMove → POST /api/nodes/move with a flat body (camelCase match)', async () => {
    await invokeCmd(COMMANDS.nodesMove, { nodeIds: ['m1', 'c1'], parentId: 'p1', index: 2 });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes/move');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      nodeIds: ['m1', 'c1'],
      parentId: 'p1',
      index: 2,
    });
  });

  it('nodesMove → a top-level drop sends parentId: null, not undefined', async () => {
    // 落到项目根是一个真实的移动目标，而 `null` 正是 `move_many` 读作「改挂到项目」的
    // 值。留成 `undefined` 会让 `JSON.stringify` 丢掉这个键，服务端就会保留旧父级。
    await invokeCmd(COMMANDS.nodesMove, { nodeIds: ['c1'], index: 0 });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.parentId, null);
    assert.deepEqual(body.nodeIds, ['c1']);
  });

  it('nodesDuplicate → POST /api/nodes/{id}/duplicate, carrying newId', async () => {
    // `newId` 是副本落库所用的 id —— SPA 乐观地铸出它，并需要服务端保留，否则它已经
    // 打开的那个页签会指向一条谁都不拥有的行。
    await invokeCmd(COMMANDS.nodesDuplicate, { id: 'n1', newId: 'copy-1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes/n1/duplicate');
    // path 吃掉了 `id`；body 投影器只交回 `newId`，好让线上载荷不带 Rust handler 会
    // 静默丢弃的 `{ id }` 字段。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { newId: 'copy-1' });
  });

  it('nodesDuplicate → no newId means "let the server mint one"', async () => {
    // 没有本地副本需要保持一致的调用方（脚本、更老的客户端）会省略它，而 handler 里的
    // `null` 意思正是如此——不是「拒绝这个请求」。
    await invokeCmd(COMMANDS.nodesDuplicate, { id: 'n1' });
    const c = last();
    assert.equal(c.url, '/api/nodes/n1/duplicate');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { newId: null });
  });

  it('endpointDetailsList → GET /api/endpoint-details with no body', async () => {
    await invokeCmd(COMMANDS.endpointDetailsList);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/endpoint-details');
    assert.equal(c.body, undefined);
  });

  it('endpointDetailGet → GET /api/nodes/{nodeId}/endpoint-detail', async () => {
    await invokeCmd(COMMANDS.endpointDetailGet, { nodeId: 'n1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/nodes/n1/endpoint-detail');
    assert.equal(c.body, undefined);
  });

  it('endpointDetailUpdate → PATCH /api/nodes/{nodeId}/endpoint-detail with body = patch only', async () => {
    await invokeCmd(COMMANDS.endpointDetailUpdate, {
      nodeId: 'n1',
      patch: { method: 'POST' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/nodes/n1/endpoint-detail');
    // 节点 id 走 path；在 body 里重复它会被 `EndpointDetailPatch` 静默丢弃。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { method: 'POST' });
  });

  it('environmentsList → GET /api/environments with projectId query', async () => {
    await invokeCmd(COMMANDS.environmentsList, { projectId: 'p1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/environments?projectId=p1');
  });

  it('environmentsSetVar → POST /api/environments/{envId}/vars with flat body', async () => {
    await invokeCmd(COMMANDS.environmentsSetVar, {
      envId: 'env 1',
      key: 'token',
      value: 'abc',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env%201/vars');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { key: 'token', value: 'abc' });
  });

  it('environmentsSetModuleBaseurl → POST /api/environments/{envId}/module-baseurls', async () => {
    await invokeCmd(COMMANDS.environmentsSetModuleBaseurl, {
      envId: 'env1',
      defId: 'def1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env1/module-baseurls');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      defId: 'def1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
    });
  });

  it('environmentsSetActive → POST /api/environments/active (literal route)', async () => {
    await invokeCmd(COMMANDS.environmentsSetActive, { envId: 'env-1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/active');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { envId: 'env-1' });
  });

  it('environmentsUpdate → PATCH /api/environments/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.environmentsUpdate, {
      id: 'env-1',
      patch: { name: 'Renamed' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/environments/env-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('environmentsRenameVar → POST /api/environments/{envId}/vars/rename with flat body', async () => {
    await invokeCmd(COMMANDS.environmentsRenameVar, {
      envId: 'env 1',
      fromKey: 'old',
      toKey: 'new',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    // 环境 id 走路径编码；重命名只需一次往返，不必先删后设。
    assert.equal(c.url, '/api/environments/env%201/vars/rename');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { fromKey: 'old', toKey: 'new' });
  });

  it('baseUrlDefsCreate → POST /api/projects/{projectId}/baseurls with { name }', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      // 服务挂在项目上，所以作用域是 projectId；顶层 `id` 不能随默认 args
      // 泄漏进 body（调用方把要用的 id 放在 opts 里）。
      id: 'def-1',
      projectId: 'p/1',
      name: 'Local',
      opts: { description: '本地服务' },
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/projects/p%2F1/baseurls');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      name: 'Local',
      description: '本地服务',
    });
  });

  it('baseUrlDefsCreate → opts.id 下发，服务行不会与 ★ / 单元格脱钩', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      projectId: 'p1',
      name: 'auth',
      opts: { id: 'def_client_1' },
    });
    const c = last();
    // 前端铸 id、后端采纳：丢掉它就等于让 ★ 和 env 单元格指向一条从未
    // 落库的行，解析必然为空。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'auth', id: 'def_client_1' });
  });

  it('baseUrlDefsCreate → opts.id 为空串时不发 id，由服务端铸', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      projectId: 'p1',
      name: 'auth',
      opts: { id: '' },
    });
    const c = last();
    // 空串不能变成一条 id 为 "" 的行。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'auth' });
  });

  it('baseUrlDefsUpdate → PATCH /api/baseurl-defs/{id}', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsUpdate, {
      id: 'def-1',
      patch: { name: 'Renamed' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/baseurl-defs/def-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('baseUrlDefsDelete → DELETE /api/baseurl-defs/{id}', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsDelete, { id: 'def-1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/baseurl-defs/def-1');
    assert.equal(c.body, undefined);
  });

  it('baseUrlDefsSetDefault → POST /api/projects/{projectId}/baseurls/default with { defId } body', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsSetDefault, {
      projectId: 'p-1',
      defId: 'd-2',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/projects/p-1/baseurls/default');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { defId: 'd-2' });
  });

  it('baseUrlDefsSetDefault → defId: null 表示该项目没有默认服务', async () => {
    // `null` 是清空 ★ 的显式动作，不是"恢复某个模块的默认值"——
    // ★ 是项目级的，根本不存在模块默认值。
    await invokeCmd(COMMANDS.baseUrlDefsSetDefault, { projectId: 'p-1', defId: null });
    const c = last();
    assert.equal(c.url, '/api/projects/p-1/baseurls/default');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { defId: null });
  });

  it('requestSettingsGet → GET /api/request-settings', async () => {
    await invokeCmd(COMMANDS.requestSettingsGet);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/request-settings');
  });

  it('requestSettingsUpdate → PUT /api/request-settings with full settings body', async () => {
    await invokeCmd(COMMANDS.requestSettingsUpdate, {
      patch: { timeoutMs: 5000, followRedirects: true, maxResponseSizeKb: 1024 },
    });
    const c = last();
    assert.equal(c.method, 'PUT');
    assert.equal(c.url, '/api/request-settings');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      timeoutMs: 5000,
      followRedirects: true,
      maxResponseSizeKb: 1024,
    });
  });

  it('permissionsGet → GET /api/permissions/{projectId}', async () => {
    await invokeCmd(COMMANDS.permissionsGet, { projectId: 'p-1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/permissions/p-1');
  });

  it('permissionsSet → PUT /api/permissions/{projectId} body { permission }', async () => {
    await invokeCmd(COMMANDS.permissionsSet, {
      projectId: 'p-1',
      permission: 'readonly',
    });
    const c = last();
    assert.equal(c.method, 'PUT');
    assert.equal(c.url, '/api/permissions/p-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { permission: 'readonly' });
  });

  it('mcpDaemonRestart → POST /api/mcp/restart', async () => {
    await invokeCmd(COMMANDS.mcpDaemonRestart);
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/mcp/restart');
  });

  it('mcpDaemonProxyPath → GET /api/mcp/proxy-path', async () => {
    await invokeCmd(COMMANDS.mcpDaemonProxyPath);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/mcp/proxy-path');
    assert.equal(c.body, undefined);
  });

  it('mcpAutoStartSet rewrites the body to { key, value }', async () => {
    await invokeCmd(COMMANDS.mcpAutoStartSet, { value: true });
    const c = last();
    assert.equal(c.method, 'PUT');
    assert.equal(c.url, '/api/preferences');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      key: 'mcp.auto_start',
      value: true,
    });
  });

  it('mcpAutoStartGet performs GET and plucks mcp.auto_start from the bulk response', async () => {
    installFetchMock({
      json: { 'mcp.auto_start': true, request_settings: { timeoutMs: 1 } },
      status: 200,
    });
    const v = await invokeCmd<boolean>(COMMANDS.mcpAutoStartGet);
    assert.equal(v, true);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/preferences');
  });

  it('mcpAutoStartGet resolves to false when the key is missing', async () => {
    installFetchMock({ json: {}, status: 200 });
    const v = await invokeCmd<boolean>(COMMANDS.mcpAutoStartGet);
    assert.equal(v, false);
  });

  it('every invoke call sets content-type: application/json', async () => {
    await invokeCmd(COMMANDS.environmentsSetModuleBaseurl, {
      envId: 'env1',
      defId: 'def1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
    });
    const c = last();
    assert.equal(c.contentType, 'application/json');
  });
});

// ---------------------------------------------------------------------------
// 把 options.signal 转发给 fetch。
// ---------------------------------------------------------------------------

describe('invokeCmd (web shape) — abort signal forwarding', () => {
  before(() => installFetchMock({ json: {}, status: 200 }));
  after(() => restoreFetchMock());

  it('forwards options.signal to fetch', async () => {
    calls = [];
    const ctrl = new AbortController();
    await invokeCmd(COMMANDS.projectsList, undefined, { signal: ctrl.signal });
    assert.ok(calls.length === 1);
    assert.ok(calls[0]!.signal, 'expected signal to be forwarded');
    // 两个引用指向同一个控制器的 signal。
    assert.ok(calls[0]!.signal === ctrl.signal);
  });
});

// ---------------------------------------------------------------------------
// 错误路径 —— 没有 `WEB_ROUTES` 条目的命令必须抛出，并同时指向两个源文件。
// ---------------------------------------------------------------------------

describe('invokeCmd (web shape) — error path', () => {
  before(() => installFetchMock({ json: {}, status: 200 }));
  after(() => restoreFetchMock());

  it('throws a self-documenting error when the command is unmapped', async () => {
    // 命令未映射时抛出自带说明的错误。
    // 经 `unknown` 强转，因为没有任何公开字面量代表未映射的命令（否则 TypeScript
    // 会在调用点就报错）。
    const fakeCmd = 'this_command_does_not_exist' as unknown as TauriCommand;
    await assert.rejects(() => invokeCmd(fakeCmd), /src\/lib\/tauri-commands\.ts/);
    await assert.rejects(() => invokeCmd(fakeCmd), /src-tauri\/src\/web_api\//);
    // `fetch` 必须**没有**被调用——这次抛错要在任何网络动作之前短路，否则来自 404 的
    // 错误会误导人。
    assert.equal(calls.length, 0);
  });
});

// ---------------------------------------------------------------------------
// 反向代理 base path。
//
// `serve-web` 可以把整个 router 嵌在一个前缀下（`--public-base-path` /
// `SIMPLE_POST_PUBLIC_BASE_PATH`），并在模块入口运行前把该值以
// `window.__SIMPLE_POST_BASE__` 注入 SPA shell。渲染层必须读它，因为每条
// `WEB_ROUTES` 路径都是根绝对的（`/api/projects`），而嵌套后的 router 从不服务裸根路径。
//
// 这一块是单测看不到的另一半：集成测试（`scripts/test-web-basepath.mjs`）断言*服务端*
// 嵌套正确，而本文件断言渲染层把命令映射到路径。两者都没断言它们实际一致的那个 URL。
// ---------------------------------------------------------------------------

describe('readApiBase', () => {
  const realWindow = (globalThis as { window?: unknown }).window;

  afterEach(() => {
    if (realWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else (globalThis as { window?: unknown }).window = realWindow;
  });

  it('is empty in Node, where there is no window at all', () => {
    delete (globalThis as { window?: unknown }).window;
    assert.equal(readApiBase(), '');
  });

  it('is empty for a root deployment that injects nothing', () => {
    (globalThis as { window?: unknown }).window = {};
    assert.equal(readApiBase(), '');
  });

  it('returns the injected prefix', () => {
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: '/sp' };
    assert.equal(readApiBase(), '/sp');
  });

  // `nest` 会去掉尾斜杠，所以运维写 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp/` 时不该产出
  // `/sp//api/...`。
  it('normalises a trailing slash so URLs do not gain a double slash', () => {
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: '/sp/' };
    assert.equal(readApiBase(), '/sp');
  });

  it('ignores a non-string injection', () => {
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: 42 };
    assert.equal(readApiBase(), '');
  });
});

describe('invokeCmd (web shape) under a reverse-proxy base path', () => {
  const realWindow = (globalThis as { window?: unknown }).window;

  beforeEach(() => {
    installFetchMock({ json: [], status: 200 });
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: '/sp' };
  });
  after(() => {
    restoreFetchMock();
    if (realWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else (globalThis as { window?: unknown }).window = realWindow;
  });

  it('requests the prefixed URL, not the bare root', async () => {
    await invokeCmd(COMMANDS.projectsList);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].url, '/sp/api/projects');
  });

  it('prefixes path params too', async () => {
    await invokeCmd(COMMANDS.projectsGet, { id: 'proj_1' });
    assert.equal(calls[0].url, '/sp/api/projects/proj_1');
  });

  it('falls back to the bare root when no prefix is injected', async () => {
    (globalThis as { window?: unknown }).window = {};
    await invokeCmd(COMMANDS.projectsList);
    assert.equal(calls[0].url, '/api/projects');
  });
});
