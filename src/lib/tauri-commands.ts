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
import { invoke } from '@tauri-apps/api/core';

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
  environmentsSetBaseurl: 'environments_set_baseurl',
  environmentsSetActive: 'environments_set_active',

  // ----- baseUrl definitions -----
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
  mcpDaemonRestart: 'mcp_daemon_restart',
  /**
   * Resolves the absolute on-disk path of the stdio proxy binary
   * (`simple-post-mcp-proxy.exe` / `simple-post-mcp-proxy`). The
   * ConnectionSection uses this to fill the "代理二进制路径"
   * placeholder with a real value instead of the literal hint.
   */
  mcpDaemonProxyPath: 'mcp_daemon_proxy_path',
  /**
   * Persisted `mcp.auto_start` preference — see
   * `docs/distribution-modes-2026-09-18.md` §5. Defaults to `false`;
   * both Tauri and `serve-web` shapes honour this row.
   */
  mcpAutoStartGet: 'mcp_auto_start_get',
  mcpAutoStartSet: 'mcp_auto_start_set',

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
 * For abortable calls (Tauri 2 doesn't accept an AbortSignal yet), wrap
 * the inner `invoke` yourself as `src/lib/http.ts` does.
 *
 * Distribution shapes — see `docs/distribution-modes-2026-09-18.md` §3:
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

export function invokeCmd<T>(
  cmd: TauriCommand,
  args?: Record<string, unknown>,
): Promise<T> {
  if (IS_TAURI_SHELL) {
    return tauriInvoke<T>(cmd, args ?? {});
  }
  return webInvokeCmd<T>(cmd, args ?? {});
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
}

/**
 * REST mapping for commands that have a browser-facing counterpart.
 * Commands without an entry here throw if invoked in the web shape —
 * that's deliberate, because surfacing a "not implemented" is much
 * less confusing than silently 404'ing.
 */
const WEB_ROUTES: Partial<Record<TauriCommand, RouteSpec>> = {
  // projects
  [COMMANDS.projectsList]: { method: 'GET', path: () => '/api/projects' },
  [COMMANDS.projectsGet]: {
    method: 'GET',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.projectsCreate]: { method: 'POST', path: () => '/api/projects' },
  [COMMANDS.projectsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.projectsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },
  // MCP daemon
  [COMMANDS.mcpStatus]: { method: 'GET', path: () => '/api/mcp/status' },
  [COMMANDS.mcpAutoStartGet]: { method: 'GET', path: () => '/api/preferences' },
  [COMMANDS.mcpAutoStartSet]: {
    method: 'PUT',
    path: () => '/api/preferences',
  },
};

async function webInvokeCmd<T>(
  cmd: TauriCommand,
  args: Record<string, unknown>,
): Promise<T> {
  const spec = WEB_ROUTES[cmd];
  if (!spec) {
    throw new Error(
      `Command "${cmd}" is not exposed in the web shape. Add a REST entry in src/lib/tauri-commands.ts + src-tauri/src/web_api.rs to enable it.`,
    );
  }
  // mcp_auto_start_set uses PUT /api/preferences with a body shape that
  // is unrelated to the Tauri command's `value: bool` arg. Translate it
  // here so the call site stays identical between shapes.
  let body: unknown = args;
  if (cmd === COMMANDS.mcpAutoStartSet) {
    body = { key: 'mcp.auto_start', value: args.value };
  } else if (cmd === COMMANDS.mcpAutoStartGet) {
    // GET /api/preferences returns the whole object; pick the key out.
    const map = await webFetch<Record<string, unknown>>('GET', spec.path(args), null);
    return (map['mcp.auto_start'] === true) as unknown as T;
  }

  return webFetch<T>(spec.method, spec.path(args), body);
}

async function webFetch<T>(
  method: RouteSpec['method'],
  path: string,
  body: unknown,
): Promise<T> {
  const init: RequestInit = {
    method,
    headers: { 'content-type': 'application/json' },
  };
  if (body !== null && method !== 'GET' && method !== 'DELETE') {
    init.body = JSON.stringify(body);
  }
  const res = await fetch(path, init);
  if (!res.ok) {
    let detail = '';
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
