/**
 * Generates the MCP client configuration snippet shown in
 * Settings → Connection.
 *
 * ## Why this module is so picky about field names
 *
 * There is no single "MCP client config schema". Every client invented
 * its own, and the differences are **silent** — a wrong root key or a
 * wrong `type` value does not raise an error, the server just never
 * appears in the client's tool list. That is the worst failure mode for
 * a "copy this config" button. The first version of this file shipped
 * one uniform `{"mcpServers": {...}}` blob with no `type` field at all,
 * which no client accepted as-is.
 *
 * ## Why there is only ONE preset now
 *
 * The table used to carry nine rows — claude-desktop, claude-code,
 * cursor, vscode, cline, windsurf, continue, zed, http-generic — and
 * every row was a hand-maintained transcription of that client's
 * documented shape. That table has been reduced to Claude Code's HTTP
 * entry, for two reasons that are worth keeping in mind before adding
 * a row back:
 *
 * 1. **The stdio columns were dead weight for this app.** The bridge
 *    binary (`simple-post-mcp`) is not installed by any of these
 *    clients' installers, and the generated `command` was a path the
 *    user has to fix by hand. HTTP needs nothing on the client machine,
 *    which is also the arrangement that keeps working across restarts
 *    (AGENTS.md: "an MCP client must not hardcode a daemon port").
 * 2. **A preset is only worth shipping when it is verified.** The
 *    removed rows each encoded a client quirk that was true at the
 *    time it was written and silently rots — Cline's `streamableHttp`,
 *    Zed's `context_servers`, VS Code's `servers`. A stale row is worse
 *    than an absent one, because the user cannot tell it is stale.
 *
 * The verified Claude Code HTTP shape, kept verbatim:
 *
 * | Client      | root key     | http                                 |
 * |-------------|--------------|--------------------------------------|
 * | Claude Code | `mcpServers` | `type: "http"` + `url`               |
 *
 * Claude Code also accepts `type: "stdio"` + command/args, but we no
 * longer generate it — see point 1 above.
 *
 * Kept as a pure module (no React, no IPC) so it is directly
 * unit-testable and the panel holds no business logic.
 */

/**
 * Runtime facts the generator needs. Nullable — the panel must render
 * something useful before the daemon has reported a port.
 */
export interface McpEndpointFacts {
  /** Port of the running daemon, from the `mcp_status` snapshot. */
  port: number | null;
}

/**
 * The only transport this module generates.
 *
 * Narrowed from `'http' | 'stdio'` when the other presets went away.
 * Nothing in the module branches on it any more, so it is kept as a
 * named type rather than deleted: it is still the contract the panel
 * renders, and it is the one place a future transport would be added.
 */
export type McpTransport = 'http';

/**
 * One AI client we generate a snippet for.
 *
 * `configPath` is a *hint* shown as prose — this app never edits a
 * third-party client's config file. `source` is the documentation the
 * shape was taken from; keep it, because these formats drift and the
 * next person to touch this needs to know whether it was guessed.
 */
export interface McpClientPreset {
  id: string;
  description: string;
  /** Where the user pastes this. Prose, may mention OS differences. */
  configPath: string;
  /**
   * Top-level key the client reads servers from.
   *
   * Narrowed from `'mcpServers' | 'servers' | 'context_servers'` with
   * the presets that needed the others.
   */
  rootKey: 'mcpServers';
  /** Value to put in `type`. Claude Code's HTTP entry requires it. */
  httpType: string;
  /** Where the shape was verified. */
  source: string;
}

/**
 * The one preset this app ships.
 *
 * An array rather than a bare object so the panel can keep rendering a
 * list — and so `MCP_CLIENT_PRESETS.length === 1` is itself an
 * assertion the test suite makes about "only one format ships".
 */
export const MCP_CLIENT_PRESETS: readonly McpClientPreset[] = [
  {
    id: 'claude-code',
    description: 'CLI / IDE 插件，项目级 .mcp.json 或用户级 ~/.claude.json',
    configPath: '.mcp.json（项目）或 ~/.claude.json（用户）',
    rootKey: 'mcpServers',
    // Claude Code writes this itself via `claude mcp add --transport http`;
    // a remote entry without `type` is not what the CLI produces.
    httpType: 'http',
    source: 'Claude Code 文档：`claude mcp add --transport http` 写出 type: "http"',
  },
] as const;

/** Server key used inside every generated snippet. */
export const MCP_SERVER_KEY = 'simple-post';

/**
 * Base URL of the running daemon's MCP endpoint, or `null` when the
 * daemon has not reported a port yet (stopped / crashed).
 */
export function daemonMcpUrl(port: number | null): string | null {
  if (port === null || !Number.isInteger(port) || port <= 0) return null;
  return `http://127.0.0.1:${port}/mcp`;
}

/** One entry of a client's server map. */
export type ServerEntry = Record<string, unknown>;

/**
 * Build the single server entry for a preset.
 *
 * Returns `null` when a required fact is missing — callers render an
 * explanatory placeholder rather than emitting a snippet with a literal
 * `null` in it, because a copyable-but-broken config is worse than an
 * honest "启动 MCP 后可用".
 */
export function buildServerEntry(
  preset: McpClientPreset,
  facts: McpEndpointFacts,
): ServerEntry | null {
  const url = daemonMcpUrl(facts.port);
  if (!url) return null;
  return { type: preset.httpType, url };
}

/** Wrap a server entry under the client's own root key. */
export function wrapForClient(
  preset: McpClientPreset,
  entry: ServerEntry,
): Record<string, unknown> {
  return { [preset.rootKey]: { [MCP_SERVER_KEY]: entry } };
}

/**
 * Full, copy-pasteable snippet for one client.
 *
 * Returns `null` when the daemon has no port yet — see
 * {@link buildServerEntry}.
 */
export function buildClientConfig(preset: McpClientPreset, facts: McpEndpointFacts): string | null {
  const entry = buildServerEntry(preset, facts);
  if (!entry) return null;
  return JSON.stringify(wrapForClient(preset, entry), null, 2);
}

/**
 * Human-readable reason a snippet is unavailable, for the placeholder UI.
 * Returning a sentence beats an empty code block because the user learns
 * what action unblocks them.
 */
export function unavailableReason(preset: McpClientPreset, facts: McpEndpointFacts): string {
  void preset;
  if (facts.port === null) return 'MCP 未运行 — 启动后即可生成配置';
  return '暂不可用';
}
