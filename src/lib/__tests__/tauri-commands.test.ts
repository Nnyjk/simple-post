/**
 * Unit tests for `src/lib/tauri-commands.ts` — the dual-shape
 * `invokeCmd` wrapper.
 *
 * Why this file exists
 * --------------------
 * The Tauri desktop shell and the browser / serve-web shape share a
 * single renderer call site (`invokeCmd`). The wrapper needs to:
 *
 *   1. Detect the shape at module load via
 *      `window.__TAURI_INTERNALS__` / `window.__TAURI__`.
 *   2. Map every invoke command name on the `COMMANDS` table to a
 *      concrete `(method, path, body)` triple for the web shape —
 *      including path-param encoding, query-string projection for
 *      list filters, and body projection when path params must be
 *      peeled off the wire payload (e.g. `projects_update`).
 *   3. Throw a self-documenting error if a command is missing from
 *      `WEB_ROUTES` so the developer is pointed at the right files
 *      to add it.
 *
 * What this file covers
 * ---------------------
 *   * `IS_TAURI_SHELL` is verified through a child Node process so
 *     the module-load-time evaluation runs in isolation (the const
 *     can't be retoggled in the same process — see the table at the
 *     top of `tauri-commands.ts`).
 *   * `invokeCmd` in the web shape is exercised by stubbing global
 *     `fetch`. We assert on `(method, url, parsed body)` so a silent
 *     drift in route paths would fail loudly.
 *   * Path-param encoding (`encodeURIComponent`) is checked on a
 *     representative collection of entry points.
 *   * Path-param + body separation (`body:` projector) is checked on
 *     `projects_update` (and a handful of others by side effect).
 *   * The unknown-cmd throw points the developer at both
 *     `tauri-commands.ts` and `web_api/`.
 *
 * Run via:
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
// IS_TAURI_SHELL — must be run in an isolated child process because the
// value is captured at module evaluation time.
// ---------------------------------------------------------------------------

/**
 * Spawn a fresh Node process that loads `tauri-commands.ts` and prints
 * `IS_TAURI_SHELL` as JSON. The optional `window` object is installed
 * on `globalThis` before the module is imported so the module-load-time
 * detection sees the right shape.
 */
function readIsTauriShellInChild(windowShim?: Record<string, unknown>): {
  value: boolean;
  stderr: string;
} {
  const setup = windowShim
    ? `globalThis.window = ${JSON.stringify(windowShim)};`
    : `delete globalThis.window;`;
  // Dynamic import in the child needs a `file://` URL on Windows; a
  // bare absolute path gets parsed as a URL and rejected.
  // `new URL(rel, import.meta.url).href` returns the file URL as a
  // string, which is the shape dynamic `import()` requires.
  // Relative to *this* file, so it needs the `../` hop out of `__tests__`.
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
    // Sanity check — guarantees the rest of the suite runs in web shape.
    assert.equal(IS_TAURI_SHELL, false);
  });
});

// ---------------------------------------------------------------------------
// invokeCmd in the web shape — path / method / body assertions.
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
    // '/' and ' ' must be percent-encoded; nothing raw should leak.
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
    // The `id` MUST NOT leak into the body — the Rust handler
    // deserialises the body into a `ProjectPatch` struct and an
    // unexpected `id` field would be silently ignored at best.
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
    // List commands are sometimes invoked with no args in renderer code
    // (`invoke<ResourceNode[]>(COMMANDS.nodesList)`); the URL must not
    // gain a trailing "?".
    await invokeCmd(COMMANDS.nodesList);
    const c = last();
    assert.equal(c.url, '/api/nodes');
  });

  it('nodesList → projects type and query onto the same canonical route', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX', type: 'endpoint', query: 'login' });
    const c = last();
    // `buildQuery` sorts keys, so the wire order is alphabetical
    // (`q` < `rootId` < `type`), not the caller's argument order.
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
    // The node's fields ARE the body. A `{ node: {…} }` wrapper would
    // fail to deserialize into `NodeCreateBody`'s `#[serde(flatten)]`.
    // `sortOrder` has no server default, so an omitted one must become
    // `0` rather than `undefined` (which `JSON.stringify` drops).
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
    // A module's parent is the project, expressed as `null`.
    assert.equal(body.parentId, null);
    assert.equal(body.sortOrder, 3);
    // `endpoint_details.node_id` is required on the wire; the client
    // already knows it, so the projection injects it rather than making
    // the caller duplicate the id.
    assert.deepEqual(body.detail, { method: 'POST', url: '/login', nodeId: 'n1' });
  });

  it('nodesUpdate → PATCH /api/nodes/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.nodesUpdate, { id: 'n1', patch: { name: 'Renamed' } });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/nodes/n1');
    // The `id` MUST NOT leak into the body — the Rust handler
    // deserialises the body into a `NodePatch` struct.
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
    // Dropping onto the project root is a real move target, and `null`
    // is what `move_many` reads as "re-parent to the project". Leaving
    // it `undefined` would let `JSON.stringify` drop the key and make
    // the server keep the old parent.
    await invokeCmd(COMMANDS.nodesMove, { nodeIds: ['c1'], index: 0 });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.parentId, null);
    assert.deepEqual(body.nodeIds, ['c1']);
  });

  it('nodesDuplicate → POST /api/nodes/{id}/duplicate, carrying newId', async () => {
    // `newId` is the id the copy is stored under — the SPA mints it
    // optimistically and needs the server to keep it, or the tab it
    // already opened points at a row nobody owns.
    await invokeCmd(COMMANDS.nodesDuplicate, { id: 'n1', newId: 'copy-1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes/n1/duplicate');
    // The path consumed `id`; the body projector hands back only `newId`
    // so the wire payload doesn't carry an `{ id }` field the Rust
    // handler would silently drop.
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { newId: 'copy-1' });
  });

  it('nodesDuplicate → no newId means "let the server mint one"', async () => {
    // A caller with no local copy to stay consistent with (a script, an
    // older client) omits it, and the handler's `null` means exactly
    // that — not "reject the request".
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
    // The node id is the path; repeating it in the body would be
    // silently dropped by `EndpointDetailPatch`.
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
    // The env id is path-encoded; the rename needs one round-trip, not
    // a delete followed by a set.
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
// Forwarding of options.signal to fetch.
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
    // Both references target the same controller's signal.
    assert.ok(calls[0]!.signal === ctrl.signal);
  });
});

// ---------------------------------------------------------------------------
// Error path — commands without a `WEB_ROUTES` entry must throw with a
// pointer to both source files.
// ---------------------------------------------------------------------------

describe('invokeCmd (web shape) — error path', () => {
  before(() => installFetchMock({ json: {}, status: 200 }));
  after(() => restoreFetchMock());

  it('throws a self-documenting error when the command is unmapped', async () => {
    // Cast through `unknown` because no public literal exists for an
    // unmapped command (TypeScript would otherwise flag it at the
    // call site).
    const fakeCmd = 'this_command_does_not_exist' as unknown as TauriCommand;
    await assert.rejects(() => invokeCmd(fakeCmd), /src\/lib\/tauri-commands\.ts/);
    await assert.rejects(() => invokeCmd(fakeCmd), /src-tauri\/src\/web_api\//);
    // Fetch must NOT have been called — the throw must short-circuit
    // before any network activity, otherwise the error from a 404
    // would be misleading.
    assert.equal(calls.length, 0);
  });
});

// ---------------------------------------------------------------------------
// Reverse-proxy base path.
//
// `serve-web` can nest its whole router under a prefix
// (`--public-base-path` / `SIMPLE_POST_PUBLIC_BASE_PATH`), and it injects
// that value into the SPA shell as `window.__SIMPLE_POST_BASE__` before
// the module entry runs. The renderer has to read it, because every
// `WEB_ROUTES` path is root-absolute (`/api/projects`) and a nested
// router never serves the bare root.
//
// This is the half the previous test file could not see: the integration
// test (`scripts/test-web-basepath.mjs`) asserted the *server* nests
// correctly, and this one asserted the renderer maps commands to paths.
// Neither asserted the URL the two of them actually agree on.
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

  // `nest` strips the trailing slash, so an operator writing
  // `SIMPLE_POST_PUBLIC_BASE_PATH=/sp/` must not produce `/sp//api/...`.
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
