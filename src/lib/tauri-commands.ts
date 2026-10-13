/**
 * Single source of truth for every Tauri command name invoked from the
 * renderer. Every `invoke(...)` call in `src/` MUST go through the
 * `COMMANDS` table — never as a bare string literal — so a rename on
 * either side (Rust registration in `src-tauri/src/lib.rs` `invoke_handler!`
 * or the renderer) is caught by the compiler / a single search-and-replace
 * instead of silently no-op'ing at runtime.
 *
 * Naming convention:
 *   - The string value MUST equal the function name in
 *     `src-tauri/src/commands/` (or the rust `#[tauri::command]` for
 *     `http_request`, which lives in `src-tauri/src/lib.rs`).
 *   - The key on the object is camelCase for ergonomic TS consumption:
 *     `mcp_status` ⇒ `mcpStatus`, `projects_update` ⇒ `projectsUpdate`.
 *   - Sort entries alphabetically within a region so a missing command
 *     shows up the moment you scan the table.
 */

export const COMMANDS = {
  // ----- projects -----
  projectsList: 'projects_list',
  projectsGet: 'projects_get',
  projectsCreate: 'projects_create',
  projectsUpdate: 'projects_update',
  projectsDelete: 'projects_delete',

  // ----- resource tree -----
  // Replaces the 19 `modules_*` / `collections_*` / `endpoints_*`
  // commands the three-table layout needed. A node is one of the three
  // types, discriminated by `type`; the tree's shape is `parentId` and
  // its owner is `rootId`. Endpoint request configuration lives behind
  // its own pair, because `method` / `url` mean nothing for a module.
  nodesList: 'nodes_list',
  nodesGet: 'nodes_get',
  nodesCreate: 'nodes_create',
  nodesUpdate: 'nodes_update',
  nodesDelete: 'nodes_delete',
  nodesMove: 'nodes_move',
  nodesDuplicate: 'nodes_duplicate',
  endpointDetailsList: 'endpoint_details_list',
  endpointDetailGet: 'endpoint_detail_get',
  endpointDetailUpdate: 'endpoint_detail_update',

  // ----- environments -----
  environmentsList: 'environments_list',
  environmentsCreate: 'environments_create',
  environmentsUpdate: 'environments_update',
  environmentsDelete: 'environments_delete',
  environmentsSetVar: 'environments_set_var',
  environmentsRenameVar: 'environments_rename_var',
  environmentsSetModuleBaseurl: 'environments_set_module_baseurl',
  environmentsSetActive: 'environments_set_active',

  // ----- baseUrl definitions -----
  baseUrlDefsList: 'base_url_defs_list',
  baseUrlDefsCreate: 'base_url_defs_create',
  baseUrlDefsUpdate: 'base_url_defs_update',
  baseUrlDefsDelete: 'base_url_defs_delete',
  baseUrlDefsSetDefault: 'base_url_defs_set_default',

  // ----- request settings -----
  requestSettingsGet: 'request_settings_get',
  requestSettingsUpdate: 'request_settings_update',

  // ----- agent permissions -----
  permissionsGet: 'permissions_get',
  permissionsSet: 'permissions_set',

  // ----- MCP daemon -----
  mcpStatus: 'mcp_status',
  /**
   * Idempotent start. If a daemon is already running this returns its
   * existing `{ok, pid, port}` without killing and respawning it — the
   * same envelope as restart, so the renderer has one shape to handle.
   */
  mcpDaemonStart: 'mcp_daemon_start',
  /** Manual runtime kill switch. Returns `{ok, pid: null, port: null}`. */
  mcpDaemonStop: 'mcp_daemon_stop',
  /**
   * Restart MUST terminate the daemon the port record points at, not just
   * the `Child` handle the supervisor happens to own: when the supervisor
   * adopted an already-running daemon it has no handle, so a
   * handle-only kill silently leaves the old process alive and the
   * "restart" adopts the same process again.
   */
  mcpDaemonRestart: 'mcp_daemon_restart',
  /**
   * Resolves the absolute on-disk path of the stdio bridge binary
   * (`simple-post-mcp.exe` / `simple-post-mcp`). The
   * ConnectionSection uses this to fill the "代理二进制路径"
   * placeholder with a real value instead of the literal hint.
   * The legacy `simple-post-mcp-proxy` binary was removed; the
   * standalone bridge binary is now `simple-post-mcp`.
   */
  mcpDaemonProxyPath: 'mcp_daemon_proxy_path',
  /**
   * Persisted `mcp.auto_start` preference. Defaults to `false`;
   * both Tauri and `serve-web` shapes honour this row.
   */
  mcpAutoStartGet: 'mcp_auto_start_get',
  mcpAutoStartSet: 'mcp_auto_start_set',

  // ----- MCP capability policy -----
  // Three-tier global capability that filters `tools/list` for AI
  // clients. Default `read_only` — the key is absent until the user picks a
  // tier in Settings, and an absent key resolves to the read-only tier.
  mcpCapabilityGet: 'mcp_capability_get',
  mcpCapabilitySet: 'mcp_capability_set',
  mcpCapabilityListTools: 'mcp_capability_list_tools',

  // ----- Streamable HTTP listener config -----
  // Host / port / path of the MCP endpoint, plus the remote-access
  // opt-in and the Host/Origin whitelists. Read returns the persisted
  // config; the write returns `{ config, requiresRestart }` — the
  // listener cannot rebind in place, so the caller surfaces a restart
  // hint rather than pretending the change is live.
  mcpHttpConfigGet: 'mcp_http_config_get',
  mcpHttpConfigSet: 'mcp_http_config_set',

  // ----- low-level HTTP executor (Tauri side) -----
  httpRequest: 'http_request',
} as const;

export type TauriCommand = (typeof COMMANDS)[keyof typeof COMMANDS];

/**
 * Typed `invoke` wrapper. Prefer this over calling `invoke` directly so
 * the command name is still traceable to the table and the return type
 * is explicit at the call site.
 *
 * Usage:
 *   const project = await invokeCmd<Project>(COMMANDS.projectsGet, { id });
 *
 * For abortable calls, pass an `AbortSignal` via the `options.signal`
 * field. The Tauri shape drops it on the floor (Tauri 2 IPC has no
 * native signal yet) — the JS-side promise still rejects on abort,
 * and the late Rust response is discarded by the caller's
 * `inFlightRef`. The web shape forwards the signal to `fetch` so the
 * browser actually cancels the network request.
 *
 * Distribution shapes:
 *   - Tauri shell  → `__TAURI_INTERNALS__` is present, calls `invoke`.
 *   - Web / Dev    → falls through to `webInvokeCmd` which hits the
 *                    axum `serve-web` host's `/api/*` REST surface.
 *
 * Module-load detection is intentional: flipping shapes within a
 * session requires a reload (the runtime is fixed when the page loads).
 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core';

export const IS_TAURI_SHELL: boolean =
  typeof window !== 'undefined' &&
  Boolean(
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ ||
    (window as { __TAURI__?: unknown }).__TAURI__,
  );

/**
 * Optional per-call options for `invokeCmd`. Currently just an abort
 * signal; structured as an object so future flags (e.g. timeout, retry
 * policy) can be added without breaking call sites.
 */
export interface CallApiOptions {
  /** Cancel the in-flight IPC call. Tauri shape: promise rejects but
   *  Rust keeps running; web shape: native fetch abort. */
  signal?: AbortSignal;
}

export function invokeCmd<T>(
  cmd: TauriCommand,
  args?: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  if (IS_TAURI_SHELL) {
    // Tauri 2's `invoke` doesn't accept an AbortSignal — drop it here
    // so the call site doesn't have to branch on shape. Callers that
    // need abortable behaviour in Tauri should wrap the promise
    // themselves (or pass `options.signal` and listen for `abort`).
    return tauriInvoke<T>(cmd, args ?? {});
  }
  return webInvokeCmd<T>(cmd, args ?? {}, options);
}

// -------------------------------------------------------------------------
// Web-shape REST bridge.
//
// The route table mirrors the axum router in `src-tauri/src/web_api/`
// (`.route()` registrations live in `mod.rs::build_router`).
// We don't share the source — the two endpoints are kept in sync by the
// integration tests in `scripts/mcp-integration.test.mjs` and a manual
// review pass at PR time. Adding a new invoke command requires adding
// the matching REST entry in BOTH places.
// -------------------------------------------------------------------------

interface RouteSpec {
  method: 'GET' | 'POST' | 'PATCH' | 'DELETE' | 'PUT';
  /** Path template; `{key}` segments get substituted from `args`. */
  path: (args: Record<string, unknown>) => string;
  /**
   * Build the request body. Default: `args` itself. Override when path
   * params must be stripped out of the body — e.g. an update where
   * `args` is `{ id, patch }` but the wire body should be `{ patch }`.
   */
  body?: (args: Record<string, unknown>) => unknown;
}

/**
 * Build a stable, deterministic `?key=value&…` query string from a
 * record. Drops `undefined` / `null` / empty-string values, sorts
 * keys, and runs both halves through `encodeURIComponent`.
 */
function buildQuery(params: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const key of Object.keys(params).sort()) {
    const raw = params[key];
    if (raw === undefined || raw === null || raw === '') continue;
    parts.push(`${encodeURIComponent(key)}=${encodeURIComponent(String(raw))}`);
  }
  return parts.length === 0 ? '' : `?${parts.join('&')}`;
}

/**
 * REST mapping for commands that have a browser-facing counterpart.
 * Commands without an entry here throw if invoked in the web shape —
 * that's deliberate, because surfacing a "not implemented" is much
 * less confusing than silently 404'ing.
 *
 * Conventions:
 *   - GET    /api/{resource}            list with optional query filter
 *                                       (e.g. `?projectId=p1`).
 *   - GET    /api/{resource}/{id}       single-item read.
 *   - POST   /api/{resource}            create; the parent id travels
 *                                       in the body so the renderer
 *                                       call site stays identical
 *                                       between shapes.
 *   - PATCH  /api/{resource}/{id}       partial update; body is
 *                                       `{ patch: … }`.
 *   - DELETE /api/{resource}/{id}       remove.
 *   - POST   /api/{resource}/move       reorder (action verb that
 *                                       doesn't fit noun CRUD).
 *   - POST   /api/{resource}/{id}/…     sub-resource action
 *                                       (duplicate, activate, …).
 */
// ----- bearer-token cache -----
//
// `serve-web` may be launched with `--password=<token>` /
// `SIMPLE_POST_WEB_PASSWORD`. The renderer has to attach
// `Authorization: Bearer <token>` to every `/api/*` and `/mcp`
// request it makes. We store the bearer in `localStorage` so it
// survives reloads, and let the SPA bootstrap pick it up from the
// URL query string on first paint:
//
//   https://host/sp/?token=<x>     ← reverse-proxy auth hand-off
//   https://host/                   ← subsequent visits; reads
//                                    localStorage
//
// Security trade-off: `localStorage` is readable by any same-origin
// XSS. This is the same exposure Postman / Insomnia / dbx accept for
// standalone clients (no per-user session). Documented for the
// the beta security review.
export const WEB_BEARER_STORAGE_KEY = 'simple-post.web.bearer';

export function writeBearerToStorage(token: string): void {
  try {
    window.localStorage.setItem(WEB_BEARER_STORAGE_KEY, token);
  } catch {
    /* private mode / quota — fall back to URL-only auth */
  }
}

export function clearBearerFromStorage(): void {
  try {
    window.localStorage.removeItem(WEB_BEARER_STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * Read the bearer out of localStorage. Returns `null` when running
 * in Node-side tests (`window` undefined) or when the user has not
 * logged in yet.
 */
export function readBearerFromStorage(): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(WEB_BEARER_STORAGE_KEY);
  } catch {
    return null;
  }
}

/**
 * One-shot bootstrap: if the URL has `?token=<x>`, persist it
 * (overwriting any existing value — the user just rotated creds)
 * and strip the query so the bearer doesn't end up in browser
 * history / Referer headers. Returns the token (or null).
 */
export function ingestBearerFromUrl(search: string = window.location.search): string | null {
  if (typeof window === 'undefined') return null;
  const params = new URLSearchParams(search);
  const candidate = params.get('token');
  if (!candidate) return null;
  writeBearerToStorage(candidate);
  // Strip the query so the bearer doesn't leak into history / referrer.
  const clean = window.location.pathname + window.location.hash;
  window.history.replaceState(null, '', clean);
  return candidate;
}

const WEB_ROUTES: Partial<Record<TauriCommand, RouteSpec>> = {
  // ----- projects -----
  [COMMANDS.projectsList]: { method: 'GET', path: () => '/api/projects' },
  [COMMANDS.projectsGet]: {
    method: 'GET',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.projectsCreate]: { method: 'POST', path: () => '/api/projects' },
  [COMMANDS.projectsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.projectsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },

  // ----- resource tree -----
  // One list call replaces the three unfiltered ones the old layout
  // issued. `rootId` is the filter the SPA always has; `type` and `q`
  // are optional and project the same endpoint the Tauri command does.
  [COMMANDS.nodesList]: {
    method: 'GET',
    path: (a) => `/api/nodes${buildQuery({ rootId: a.rootId, type: a.type, q: a.query })}`,
  },
  [COMMANDS.nodesGet]: {
    method: 'GET',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.nodesCreate]: {
    method: 'POST',
    path: () => '/api/nodes',
    // `NodeCreateBody` flattens the node, so the node's own fields are
    // the body — not a `{ node: {…} }` wrapper. The Tauri command takes
    // the same values as flat args, so the projection only has to add
    // what the server stamps for itself: the id, and the detail row's
    // key. `sortOrder` is required on the wire with no default, so an
    // omitted one has to become 0 rather than `undefined`.
    //
    // `kind` — not `type` — because that is the domain field name on
    // both sides. It has to match the Tauri command's `kind` parameter
    // as well, or desktop mode asks for a key the handler never reads.
    body: (a) => ({
      id: a.id,
      rootId: a.rootId,
      parentId: a.parentId ?? null,
      kind: a.kind,
      name: a.name,
      description: a.description,
      sortOrder: a.sortOrder ?? 0,
      ...(a.detail ? { detail: { ...(a.detail as object), nodeId: a.id } } : {}),
    }),
  },
  [COMMANDS.nodesUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.nodesDelete]: {
    method: 'DELETE',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.nodesMove]: {
    method: 'POST',
    path: () => '/api/nodes/move',
    // Tauri 与 Web 使用同一个扁平参数对象；只投影 REST 所需的字段。
    // `parentId: null` is meaningful — "top of the project", legal only
    // for a module — so it is passed through rather than dropped.
    body: (a) => ({ nodeIds: a.nodeIds, parentId: a.parentId ?? null, index: a.index }),
  },
  [COMMANDS.nodesDuplicate]: {
    method: 'POST',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}/duplicate`,
    // The path consumes `id`, so the body is projected explicitly rather
    // than letting the default `body = args` leak `{ id }` into the wire.
    // `newId` is the id the copy is stored under — the SPA mints it
    // optimistically and needs the server to keep it. `null` is the
    // "server picks" fallback the handler already understands.
    body: (a) => ({ newId: a.newId ?? null }),
  },
  [COMMANDS.endpointDetailsList]: { method: 'GET', path: () => '/api/endpoint-details' },
  [COMMANDS.endpointDetailGet]: {
    method: 'GET',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.nodeId))}/endpoint-detail`,
  },
  [COMMANDS.endpointDetailUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.nodeId))}/endpoint-detail`,
    body: (a) => a.patch,
  },

  // ----- environments -----
  [COMMANDS.environmentsList]: {
    method: 'GET',
    path: (a) => `/api/environments${buildQuery({ projectId: a.projectId })}`,
  },
  [COMMANDS.environmentsCreate]: {
    method: 'POST',
    path: () => '/api/environments',
  },
  [COMMANDS.environmentsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.environmentsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.environmentsSetVar]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/vars`,
    body: (a) => ({ key: a.key, value: a.value }),
  },
  [COMMANDS.environmentsRenameVar]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/vars/rename`,
    body: (a) => ({ fromKey: a.fromKey, toKey: a.toKey }),
  },
  [COMMANDS.environmentsSetModuleBaseurl]: {
    method: 'POST',
    // The value layer is (环境 × 模块 × 服务), so both halves of the cell
    // travel in the body. The literal path segment keeps it distinct from
    // the retired `/baseurls` route, which is gone because a bare
    // `defId` could only ever have written the flat, module-blind layer.
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/module-baseurls`,
    body: (a) => ({ defId: a.defId, moduleId: a.moduleId, url: a.url }),
  },
  [COMMANDS.environmentsSetActive]: {
    method: 'POST',
    // 使用字面量 active 路径，避免与参数路由发生匹配歧义。
    path: () => '/api/environments/active',
    body: (a) => ({ envId: a.envId }),
  },

  // ----- baseUrl definitions -----
  [COMMANDS.baseUrlDefsList]: {
    method: 'GET',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls`,
  },
  [COMMANDS.baseUrlDefsCreate]: {
    method: 'POST',
    // Services hang off a **project**, so the project is the scope and
    // lives in the path; `/api/baseurl-defs` is the collection the sibling
    // PATCH/DELETE below address by bare id.
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls`,
    // Explicit projection: `name` is the whole contract, and the id is
    // minted by the server — the default `args` fallback would leak both
    // `projectId` and any caller-held `opts` into the body.
    body: (a) => {
      const opts = a.opts as { description?: unknown } | undefined;
      return {
        name: a.name,
        ...(opts ? { description: opts.description } : {}),
      };
    },
  },
  [COMMANDS.baseUrlDefsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/baseurl-defs/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.baseUrlDefsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/baseurl-defs/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.baseUrlDefsSetDefault]: {
    method: 'POST',
    // The ★ is a property of the project, so the project — not a module —
    // is the scope.
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls/default`,
    body: (a) => ({ defId: a.defId }),
  },

  // ----- request settings -----
  // The web shape exposes a dedicated `/api/request-settings` endpoint
  // (full replace semantics on PUT). The Tauri command takes a
  // `patch`; see the `assumptions` section of the deliverable for the
  // caller-side merge requirement.
  [COMMANDS.requestSettingsGet]: {
    method: 'GET',
    path: () => '/api/request-settings',
  },
  [COMMANDS.requestSettingsUpdate]: {
    method: 'PUT',
    path: () => '/api/request-settings',
    body: (a) => (a as { patch?: unknown }).patch ?? a,
  },

  // ----- agent permissions -----
  [COMMANDS.permissionsGet]: {
    method: 'GET',
    path: (a) => `/api/permissions/${encodeURIComponent(String(a.projectId))}`,
  },
  [COMMANDS.permissionsSet]: {
    method: 'PUT',
    path: (a) => `/api/permissions/${encodeURIComponent(String(a.projectId))}`,
    body: (a) => ({ permission: a.permission }),
  },

  // ----- low-level HTTP executor -----
  // Same arg shape on both sides (camelCase); the Rust `HttpSendBody`
  // struct deserialises directly from `args` so no translation is
  // needed here.
  [COMMANDS.httpRequest]: { method: 'POST', path: () => '/api/http/send' },

  // ----- MCP daemon -----
  [COMMANDS.mcpStatus]: { method: 'GET', path: () => '/api/mcp/status' },
  // The three lifecycle verbs share one `{ ok, pid, port }` envelope, so
  // the renderer's discriminated-union handling stays in one place.
  // `start` is idempotent on the server — it returns the existing daemon
  // rather than respawning it.
  [COMMANDS.mcpDaemonStart]: {
    method: 'POST',
    path: () => '/api/mcp/start',
  },
  [COMMANDS.mcpDaemonStop]: {
    method: 'POST',
    path: () => '/api/mcp/stop',
  },
  [COMMANDS.mcpDaemonRestart]: {
    method: 'POST',
    path: () => '/api/mcp/restart',
  },
  [COMMANDS.mcpDaemonProxyPath]: {
    method: 'GET',
    path: () => '/api/mcp/proxy-path',
  },
  // Pref-backed, like `request_settings_*` above.
  [COMMANDS.mcpAutoStartGet]: {
    method: 'GET',
    path: () => '/api/preferences',
  },
  [COMMANDS.mcpAutoStartSet]: {
    method: 'PUT',
    path: () => '/api/preferences',
    body: (a) => ({ key: 'mcp.auto_start', value: a.value }),
  },
  // ----- MCP capability policy -----
  // Dedicated endpoints — returns the policy directly (no need to dig
  // through the full preferences map, unlike `mcpAutoStart*`).
  [COMMANDS.mcpCapabilityGet]: {
    method: 'GET',
    path: () => '/api/mcp/capability',
  },
  [COMMANDS.mcpCapabilitySet]: {
    method: 'PUT',
    path: () => '/api/mcp/capability',
    body: (a) => ({ policy: a.policy }),
  },
  [COMMANDS.mcpCapabilityListTools]: {
    method: 'GET',
    path: () => '/api/mcp/capability/tools',
  },

  // ----- Streamable HTTP listener config -----
  // Both verbs share one path. The settings panel is reachable in the
  // `serve-web` shape too, so this pair must exist here or the section
  // silently does nothing in browser mode (dual-transport invariant).
  [COMMANDS.mcpHttpConfigGet]: {
    method: 'GET',
    path: () => '/api/mcp/http-config',
  },
  [COMMANDS.mcpHttpConfigSet]: {
    method: 'PUT',
    path: () => '/api/mcp/http-config',
    body: (a) => ({ config: a.config }),
  },
};

async function webInvokeCmd<T>(
  cmd: TauriCommand,
  args: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  const spec = WEB_ROUTES[cmd];
  if (!spec) {
    throw new Error(
      `Command "${cmd}" is not exposed in the web shape. Add a REST entry in src/lib/tauri-commands.ts + src-tauri/src/web_api/ to enable it.`,
    );
  }
  // `mcpAutoStartGet` keeps its old GET-then-pick shape because the
  // web endpoint still returns the full preferences map and the
  // renderer only wants the `mcp.auto_start` row. `requestSettingsGet`
  // gets a dedicated endpoint now (`/api/request-settings`), so no
  // special handling is needed for it.
  if (cmd === COMMANDS.mcpAutoStartGet) {
    const map = await webFetch<Record<string, unknown>>('GET', spec.path(args), null, options);
    return (map['mcp.auto_start'] === true) as unknown as T;
  }

  const body = spec.body ? spec.body(args) : args;
  return webFetch<T>(spec.method, spec.path(args), body, options);
}

async function webFetch<T>(
  method: RouteSpec['method'],
  path: string,
  body: unknown,
  options?: CallApiOptions,
): Promise<T> {
  const headers: Record<string, string> = { 'content-type': 'application/json' };
  // When the server is launched with `--password=…` /
  // `SIMPLE_POST_WEB_PASSWORD`, the renderer's `webFetch` must
  // attach `Authorization: Bearer <token>` on every API call.
  // The bearer is cached in `localStorage` (see comment block on
  // `WEB_BEARER_STORAGE_KEY`); on a fresh page load, the SPA shell
  // looks for `?token=<x>` in the URL and writes it to localStorage
  // before any fetch fires. We accept the XSS surface as a known
  // trade-off — same posture as Postman / Insomnia / dbx, all of
  // which read tokens from localStorage or extension storage.
  const bearer = readBearerFromStorage();
  if (bearer) {
    headers['authorization'] = `Bearer ${bearer}`;
  }
  const init: RequestInit = {
    method,
    headers,
  };
  if (body !== null && method !== 'GET' && method !== 'DELETE') {
    init.body = JSON.stringify(body);
  }
  if (options?.signal) {
    init.signal = options.signal;
  }
  const res = await fetch(path, init);
  if (!res.ok) {
    // Both branches assign before use — declare without an initializer
    // so `no-useless-assignment` stays happy.
    let detail: string;
    try {
      const j = (await res.json()) as { code?: string; message?: string };
      detail = ` [${j.code ?? '?'}] ${j.message ?? ''}`;
    } catch {
      detail = ` ${await res.text().catch(() => '')}`;
    }
    throw new Error(`HTTP ${res.status} ${method} ${path}${detail}`);
  }
  // 204 No Content (DELETE, PUT-pref) → return undefined cast to T.
  if (res.status === 204) return undefined as unknown as T;
  return (await res.json()) as T;
}
