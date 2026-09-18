/**
 * Renderer-facing MCP daemon state types. Lives in `src/lib/` (a plain
 * `.ts` file) so the store, the panel component, and the
 * `useMcpStatus` hook can all import the same canonical type without
 * dragging `.tsx` into `stores/` or producing a circular dependency
 * through the panel.
 *
 * The shape mirrors what `McpStatusDto` serialises from Rust
 * (see `src-tauri/src/commands.rs`). Mismatches are tolerated on the
 * renderer side: unknown lifecycle strings collapse to `'stopped'` in
 * the badge, never throw.
 */

/**
 * Lifecycle status of the MCP daemon. Mirrors the lifecycle documented
 * in the plan (running / restarting / crashed / unhealthy) and is
 * driven from `mcp_status`. Unknown values fall back to `'stopped'`.
 */
export type McpRuntimeStatus =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'restarting'
  | 'crashed'
  | 'unhealthy';

/**
 * UI projection of `mcp_status`. Fields the renderer cares about:
 *
 *   - `status` — drives the badge + the crashed dialog trigger
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
  pid: number | null;
  port: number | null;
  uptimeMs: number;
  requestsHandled: number;
  currentConnections: number;
  registeredTools: string[];
  lastError?: string;
}
