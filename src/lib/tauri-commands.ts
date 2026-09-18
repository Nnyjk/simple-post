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
 */
export function invokeCmd<T>(
  cmd: TauriCommand,
  args?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>(cmd, args ?? {});
}
