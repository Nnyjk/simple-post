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
 *     `src-tauri/src/commands.rs` (or the rust `#[tauri::command]` for
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

  // ----- modules -----
  modulesList: 'modules_list',
  modulesCreate: 'modules_create',
  modulesUpdate: 'modules_update',
  modulesDelete: 'modules_delete',
  modulesMove: 'modules_move',

  // ----- collections -----
  collectionsList: 'collections_list',
  collectionsCreate: 'collections_create',
  collectionsUpdate: 'collections_update',
  collectionsDelete: 'collections_delete',
  collectionsMove: 'collections_move',

  // ----- endpoints -----
  endpointsList: 'endpoints_list',
  endpointsGet: 'endpoints_get',
  endpointsSearch: 'endpoints_search',
  endpointsCreate: 'endpoints_create',
  endpointsUpdate: 'endpoints_update',
  endpointsDelete: 'endpoints_delete',
  endpointsDuplicate: 'endpoints_duplicate',
  endpointsMove: 'endpoints_move',

  // ----- environments -----
  environmentsList: 'environments_list',
  environmentsCreate: 'environments_create',
  environmentsUpdate: 'environments_update',
  environmentsDelete: 'environments_delete',
  environmentsSetVar: 'environments_set_var',
  environmentsRenameVar: 'environments_rename_var',
  environmentsSetBaseurl: 'environments_set_baseurl',
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

  // ----- db status -----
  // Unix-ms timestamp of the last destructive schema rebuild, or null.
  // Read once during bootstrap so a reset workspace announces itself —
  // an empty tree with no explanation is indistinguishable from never
  // having had any projects.
  dbRecreatedAt: 'db_recreated_at',

  // ----- MCP daemon -----
  mcpStatus: 'mcp_status',
  mcpDaemonRestart: 'mcp_daemon_restart',
  /**
   * Resolves the absolute on-disk path of the stdio bridge binary
   * (`simple-post-mcp.exe` / `simple-post-mcp`). The
   * ConnectionSection uses this to fill the "代理二进制路径"
   * placeholder with a real value instead of the literal hint.
   * The legacy `simple-post-mcp-proxy` binary was removed in
   * commit 3 — see plan 工作流 4 (commit 4).
   */
  mcpDaemonProxyPath: 'mcp_daemon_proxy_path',
  /**
   * Persisted `mcp.auto_start` preference. Defaults to `false`;
   * both Tauri and `serve-web` shapes honour this row.
   */
  mcpAutoStartGet: 'mcp_auto_start_get',
  mcpAutoStartSet: 'mcp_auto_start_set',

  // ----- MCP capability policy (commit 1) -----
  // Three-tier global capability that filters `tools/list` for AI
  // clients. Default `read_only` — the key is absent until the user picks a
  // tier in Settings, and an absent key resolves to the read-only tier.
  mcpCapabilityGet: 'mcp_capability_get',
  mcpCapabilitySet: 'mcp_capability_set',
  mcpCapabilityListTools: 'mcp_capability_list_tools',

  // ----- low-level HTTP executor (Tauri side) -----
  httpRequest: 'http_request',

  // ----- endpoint-scoped send -----
  // The renderer names an endpoint, never a URL. The server resolves the
  // base URL + path + `{{var}}` substitution + auth, so the browser
  // cannot point the server at an arbitrary target. `httpRequest` above
  // stays as the explicit raw-URL escape hatch.
  endpointSend: 'endpoint_send',
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
// The route table mirrors the axum router in `src-tauri/src/web_api.rs`.
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
// ----- bearer-token cache (commit 5) -----
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
// commit-5 review checklist.
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

  // ----- modules -----
  [COMMANDS.modulesList]: {
    method: 'GET',
    path: (a) => `/api/modules${buildQuery({ projectId: a.projectId })}`,
  },
  [COMMANDS.modulesCreate]: { method: 'POST', path: () => '/api/modules' },
  [COMMANDS.modulesUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/modules/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.modulesDelete]: {
    method: 'DELETE',
    path: (a) => `/api/modules/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.modulesMove]: { method: 'POST', path: () => '/api/modules/move' },

  // ----- collections -----
  [COMMANDS.collectionsList]: {
    method: 'GET',
    path: (a) => `/api/collections${buildQuery({ moduleId: a.moduleId })}`,
  },
  [COMMANDS.collectionsCreate]: {
    method: 'POST',
    path: () => '/api/collections',
  },
  [COMMANDS.collectionsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/collections/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.collectionsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/collections/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.collectionsMove]: {
    method: 'POST',
    path: () => '/api/collections/move',
    // Tauri 与 Web 使用同一个扁平目标对象；只投影 REST 所需的字段。
    body: (a) => {
      const target = a.target as {
        parentKind: 'module' | 'collection';
        parentId: string;
        index: number;
      };
      return {
        collectionIds: a.collectionIds,
        parentKind: target.parentKind,
        parentId: target.parentId,
        index: target.index,
      };
    },
  },

  // ----- endpoints -----
  [COMMANDS.endpointsList]: {
    method: 'GET',
    // canonical endpoint 列表只按 collectionId 过滤；projectId/moduleId
    // 不是 REST 端支持的筛选条件，必须在 Web 投影层丢弃，不能静默透传。
    path: (a) => `/api/endpoints${buildQuery({ collectionId: a.collectionId })}`,
  },
  [COMMANDS.endpointsGet]: {
    method: 'GET',
    path: (a) => `/api/endpoints/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.endpointsSearch]: {
    method: 'POST',
    path: () => '/api/endpoints/search',
    body: (a) => ({ query: a.query, method: a.method, tags: a.tags }),
  },
  [COMMANDS.endpointsCreate]: {
    method: 'POST',
    path: () => '/api/endpoints',
  },
  [COMMANDS.endpointsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/endpoints/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.endpointsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/endpoints/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.endpointsDuplicate]: {
    method: 'POST',
    path: (a) => `/api/endpoints/${encodeURIComponent(String(a.id))}/duplicate`,
    // The path consumes `id`; the Rust handler takes no body, so we
    // explicitly hand back an empty object rather than letting the
    // default `body = args` projection leak `{ id }` into the wire.
    body: () => ({}),
  },
  [COMMANDS.endpointsMove]: {
    method: 'POST',
    path: () => '/api/endpoints/move',
    // Rust `EndpointMoveBody` is `{ endpoint_ids, collection_id, index }`;
    // the Tauri side nests the destination in `target.collectionId`.
    body: (a) => ({
      endpointIds: a.endpointIds,
      collectionId: (a.target as { collectionId: string } | undefined)?.collectionId,
      index: (a.target as { index: number } | undefined)?.index ?? 0,
    }),
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
  [COMMANDS.environmentsSetBaseurl]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/baseurls`,
    body: (a) => ({ defId: a.defId, url: a.url }),
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
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls`,
    // projectId 只存在于路径，id 由 Web 服务端生成；显式投影可避免两者
    // 随默认 args 泄漏到请求体。
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
  [COMMANDS.dbRecreatedAt]: {
    method: 'GET',
    path: () => '/api/db/recreated-at',
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

  // Endpoint-scoped send. `endpointId` is the path segment; the Rust
  // handler folds the body back onto it before deserialising
  // `EndpointSendArgs`, so the two shapes take the same argument object.
  [COMMANDS.endpointSend]: {
    method: 'POST',
    path: (a) => `/api/endpoints/${encodeURIComponent(String(a.endpointId))}/send`,
  },

  // ----- MCP daemon -----
  [COMMANDS.mcpStatus]: { method: 'GET', path: () => '/api/mcp/status' },
  // Worker 1b added a dedicated `/api/mcp/restart` route on the
  // `web_api.rs` side; the Tauri-side restart function returns the
  // same `{ ok, pid, port }` envelope as `/start`, so reusing the
  // restart route keeps the renderer's discriminated-union handling
  // simple.
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
  // ----- MCP capability policy (commit 1) -----
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
};

async function webInvokeCmd<T>(
  cmd: TauriCommand,
  args: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  const spec = WEB_ROUTES[cmd];
  if (!spec) {
    throw new Error(
      `Command "${cmd}" is not exposed in the web shape. Add a REST entry in src/lib/tauri-commands.ts + src-tauri/src/web_api.rs to enable it.`,
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
  // commit 5: when the server is launched with `--password=…` /
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
