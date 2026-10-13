/**
 * Renderer-facing MCP daemon state types. Lives in `src/lib/` (a plain
 * `.ts` file) so the store, the panel component, and the
 * `useMcpStatus` hook can all import the same canonical type without
 * dragging `.tsx` into `stores/` or producing a circular dependency
 * through the panel.
 *
 * The shape mirrors what `McpStatusDto` serialises from Rust
 * (see `src-tauri/src/commands/`). Mismatches are tolerated on the
 * renderer side: unknown lifecycle strings collapse to `'stopped'` in
 * the badge, never throw.
 */

/**
 * Lifecycle status of the MCP daemon. Mirrors the lifecycle documented
 * in the plan (running / restarting / crashed / unhealthy) and is
 * driven from `mcp_status`. Unknown values fall back to `'stopped'`.
 */
export type McpRuntimeStatus =
  'stopped' | 'starting' | 'running' | 'restarting' | 'crashed' | 'unhealthy';

/**
 * UI projection of `mcp_status`. Fields the renderer cares about:
 *
 *   - `status` — drives the badge + the crashed dialog trigger
 *   - `running` — derived boolean kept for back-compat with any older
 *     consumer that still reads it; equals `status === 'running'`
 *   - `pid` / `port` — display only
 *   - `uptimeMs` — how long the daemon has been up
 *   - `requestsHandled` — cumulative counter
 *   - `currentConnections` — stdio/streamable-http fan-in
 *   - `registeredTools` — what the MCP server has advertised; replaces
 *     the hard-coded `REGISTERED_TOOLS` constant in ConnectionSection
 *   - `lastError` — surfaced in the crashed dialog description
 */
export interface McpUiState {
  status: McpRuntimeStatus;
  running: boolean;
  pid: number | null;
  port: number | null;
  uptimeMs: number;
  requestsHandled: number;
  currentConnections: number;
  registeredTools: string[];
  lastError?: string;
}

/**
 * Wire shape returned by the `mcp_daemon_start` / `mcp_daemon_stop` /
 * `mcp_daemon_restart` commands. All three lifecycle verbs share ONE
 * envelope on purpose: the Rust commands swallow the underlying
 * `AppError` and report a uniform result, so the renderer keeps a single
 * discriminated branch instead of a try/catch (or three different shapes)
 * at every call site.
 *
 *   - `ok: true` with a `pid`/`port` → that daemon is the live one.
 *   - `ok: true` with `pid: null`   → a legitimate stop; nothing is running.
 *   - `ok: false`                   → spawn failed / healthz timed out.
 *
 * `pid` / `port` after a successful start are the NEW daemon's identity.
 * After an idempotent start against an already-running daemon they are the
 * EXISTING daemon's — the server does not respawn in that case.
 */
export interface RestartResult {
  ok: boolean;
  pid?: number | null;
  port?: number | null;
}

/**
 * The same envelope under a name that reads correctly at the start/stop
 * call sites. Kept as an alias rather than a second interface so the
 * structural type is identical in both directions.
 */
export type DaemonControlResult = RestartResult;
