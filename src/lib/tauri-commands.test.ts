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
 *     `tauri-commands.ts` and `web_api.rs`.
 *
 * Run via:
 *   npx tsx --test src/lib/tauri-commands.test.ts
 */

import { describe, it, before, after, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';

import {
  COMMANDS,
  IS_TAURI_SHELL,
  TauriCommand,
  invokeCmd,
} from './tauri-commands';

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

function installFetchMock(
  respondWith: { json?: unknown; status?: number } = {},
): void {
  calls = [];
  const status = respondWith.status ?? 200;
  const json = respondWith.json;
  globalThis.fetch = (async (
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> => {
    const url = typeof input === 'string' ? input : input.toString();
    let bodyStr: string | undefined;
    if (typeof init?.body === 'string') bodyStr = init.body;
    const contentType = init?.headers
      ? new Headers(init.headers).get('content-type') ?? undefined
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
function readIsTauriShellInChild(
  windowShim?: Record<string, unknown>,
): { value: boolean; stderr: string } {
  const setup = windowShim
    ? `globalThis.window = ${JSON.stringify(windowShim)};`
    : `delete globalThis.window;`;
  // Dynamic import in the child needs a `file://` URL on Windows; a
  // bare absolute path gets parsed as a URL and rejected.
  // `new URL(rel, import.meta.url).href` returns the file URL as a
  // string, which is the shape dynamic `import()` requires.
  const moduleUrl = new URL('./tauri-commands.ts', import.meta.url).href;
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
    throw new Error(
      `child failed (status=${result.status}): ${result.stderr}`,
    );
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

  it('modulesList → GET /api/modules with projectId in query string', async () => {
    await invokeCmd(COMMANDS.modulesList, { projectId: 'pX' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/modules?projectId=pX');
    assert.equal(c.body, undefined);
  });

  it('modulesList (no filter) → GET /api/modules (no query string)', async () => {
    // List commands are sometimes invoked with no args in renderer code
    // (`invoke<Module[]>(COMMANDS.modulesList)`); the URL must not
    // gain a trailing "?".
    await invokeCmd(COMMANDS.modulesList);
    const c = last();
    assert.equal(c.url, '/api/modules');
  });

  it('modulesMove → POST /api/modules/move with default args body (camelCase match)', async () => {
    await invokeCmd(COMMANDS.modulesMove, { moduleId: 'm1', targetIndex: 2 });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/modules/move');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      moduleId: 'm1',
      targetIndex: 2,
    });
  });

  it('collectionsList → GET /api/collections with moduleId query', async () => {
    await invokeCmd(COMMANDS.collectionsList, { moduleId: 'mod-1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/collections?moduleId=mod-1');
  });

  it('collectionsMove → POST /api/collections/move translates nested target into flat fields', async () => {
    await invokeCmd(COMMANDS.collectionsMove, {
      collectionIds: ['c1', 'c2'],
      target: { parentKind: 'module', moduleId: 'm1', index: 1 },
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/collections/move');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      collectionIds: ['c1', 'c2'],
      parentKind: 'module',
      parentId: 'm1',
      index: 1,
    });
  });

  it('collectionsMove → uses collectionId as parentId when reparenting under a sibling collection', async () => {
    await invokeCmd(COMMANDS.collectionsMove, {
      collectionIds: ['c1'],
      target: { parentKind: 'collection', collectionId: 'c-sib', index: 0 },
    });
    const c = last();
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      collectionIds: ['c1'],
      parentKind: 'collection',
      parentId: 'c-sib',
      index: 0,
    });
  });

  it('endpointsList → GET /api/endpoints with collectionId query', async () => {
    await invokeCmd(COMMANDS.endpointsList, { collectionId: 'col-1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/endpoints?collectionId=col-1');
  });

  it('endpointsSearch → POST /api/endpoints/search with { query } body', async () => {
    await invokeCmd(COMMANDS.endpointsSearch, { query: 'login' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/endpoints/search');
    // Optional fields (`method`, `tags`) are dropped by `JSON.stringify`
    // when `undefined`, so we only assert on what survives the wire.
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { query: 'login' });
  });

  it('endpointsDuplicate → POST /api/endpoints/{id}/duplicate with empty body', async () => {
    await invokeCmd(COMMANDS.endpointsDuplicate, { id: 'e1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/endpoints/e1/duplicate');
    // The path consumed `id`; the body projector hands back `{}` so the
    // wire payload doesn't carry an `{ id }` field that the Rust
    // handler would silently drop.
    assert.equal(c.body, '{}');
  });

  it('endpointsMove → POST /api/endpoints/move translates target.collectionId into flat body', async () => {
    await invokeCmd(COMMANDS.endpointsMove, {
      endpointIds: ['e1', 'e2'],
      target: { collectionId: 'col-1', index: 0 },
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/endpoints/move');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      endpointIds: ['e1', 'e2'],
      collectionId: 'col-1',
      index: 0,
    });
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
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      envId: 'env 1',
      key: 'token',
      value: 'abc',
    });
  });

  it('environmentsSetBaseurl → POST /api/environments/{envId}/baseurls', async () => {
    await invokeCmd(COMMANDS.environmentsSetBaseurl, {
      envId: 'env1',
      defId: 'def1',
      url: 'https://api.example.com',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env1/baseurls');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      defId: 'def1',
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

  it('environmentsUpdate → PATCH /api/environments/{envId} with body = patch only', async () => {
    await invokeCmd(COMMANDS.environmentsUpdate, {
      envId: 'env-1',
      patch: { name: 'Renamed' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/environments/env-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('baseUrlDefsCreate → POST /api/projects/{projectId}/baseurls with full args body', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      projectId: 'p-1',
      id: 'def-1',
      name: 'Local',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/projects/p-1/baseurls');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      projectId: 'p-1',
      id: 'def-1',
      name: 'Local',
    });
  });

  it('baseUrlDefsUpdate → PATCH /api/baseurl-defs/{defId}', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsUpdate, {
      defId: 'def-1',
      patch: { name: 'Renamed' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/baseurl-defs/def-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('baseUrlDefsDelete → DELETE /api/baseurl-defs/{defId}', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsDelete, { defId: 'def-1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/baseurl-defs/def-1');
    assert.equal(c.body, undefined);
  });

  it('baseUrlDefsSetDefault → POST /api/projects/{pid}/baseurls/default with { defId } body', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsSetDefault, {
      projectId: 'p/1',
      defId: 'd-2',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/projects/p%2F1/baseurls/default');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { defId: 'd-2' });
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
      json: { 'mcp.auto_start': true, 'request_settings': { timeoutMs: 1 } },
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
    await invokeCmd(COMMANDS.environmentsSetBaseurl, {
      id: 'env1',
      defId: 'def1',
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
    await assert.rejects(
      () => invokeCmd(fakeCmd),
      /src\/lib\/tauri-commands\.ts/,
    );
    await assert.rejects(
      () => invokeCmd(fakeCmd),
      /src-tauri\/src\/web_api\.rs/,
    );
    // Fetch must NOT have been called — the throw must short-circuit
    // before any network activity, otherwise the error from a 404
    // would be misleading.
    assert.equal(calls.length, 0);
  });
});
