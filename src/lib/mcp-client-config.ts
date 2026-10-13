/**
 * Generates the MCP client configuration snippets shown in
 * Settings → Connection.
 *
 * ## Why this module is so picky about field names
 *
 * There is no single "MCP client config schema". Every client invented
 * its own, and the differences are **silent** — a wrong root key or a
 * wrong `type` value does not raise an error, the server just never
 * appears in the client's tool list. That is the worst failure mode for
 * a "copy this config" button, and it is what the previous version of
 * this file shipped: one uniform `{"mcpServers": {...}}` blob with no
 * `type` field at all, which no client accepted as-is.
 *
 * The verified shapes (see the doc comment on each preset for the
 * source):
 *
 * | Client        | root key        | stdio                          | http                                   |
 * |---------------|-----------------|--------------------------------|----------------------------------------|
 * | Claude Code   | `mcpServers`    | `type: "stdio"` + command/args | `type: "http"` + url                   |
 * | Claude Desktop| `mcpServers`    | `type: "stdio"` + command/args | `type: "http"` + url                   |
 * | Cursor        | `mcpServers`    | `type: "stdio"` **required**   | `url` (no type — inferred from `url`)  |
 * | Cline         | `mcpServers`    | command/args                   | `type: "streamableHttp"` **required** |
 * | Windsurf      | `mcpServers`    | command/args                   | `url`                                   |
 * | VS Code       | `servers`       | command/args (no type)         | `type: "http"` + url                    |
 * | Zed           | `context_servers` | `source:"custom"` + command/args + env | `source:"custom"` + url        |
 *
 * Two traps worth naming explicitly:
 *
 * - **Cline's type value is `streamableHttp`, not `http`.** Omit `type`
 *   on a Cline remote entry and Cline falls back to the legacy HTTP+SSE
 *   transport, which fails against a Streamable-HTTP-only server.
 * - **Zed has no `type` field at all** — it infers the transport from
 *   whether the entry has `command` or `url`, and adding `type` there
 *   produces a settings file it rejects.
 *
 * Where a client's remote shape is genuinely ambiguous across sources
 * (Continue's array-of-`transport` legacy form vs its newer object
 * form), the preset advertises stdio only rather than shipping a guess.
 * A copyable-but-wrong config is worse than an honest "not supported".
 *
 * Kept as a pure module (no React, no IPC) so it is directly
 * unit-testable and the panel holds no business logic.
 */

/** Runtime facts the generator needs. All are nullable — the panel must
 *  render something useful before the daemon has reported a port. */
export interface McpEndpointFacts {
  /** Port of the running daemon, from the `mcp_status` snapshot. */
  port: number | null;
  /** Absolute path of the `simple-post-mcp` stdio bridge binary. */
  proxyPath: string | null;
}

export type McpTransport = 'http' | 'stdio';

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
  name: string;
  description: string;
  /** Where the user pastes this. Prose, may mention OS differences. */
  configPath: string;
  /** Top-level key the client reads servers from. */
  rootKey: 'mcpServers' | 'servers' | 'context_servers';
  /**
   * Value to put in `type`, or `null` to omit the field entirely.
   * Per client, because the values genuinely differ — see the table in
   * the module doc-comment.
   */
  stdioType: string | null;
  httpType: string | null;
  /** Zed requires `source: "custom"` on each entry; nobody else wants it. */
  needsSourceField?: boolean;
  /** Transports this client is documented to accept. */
  supports: readonly McpTransport[];
  /** Where the shape was verified. */
  source: string;
}

export const MCP_CLIENT_PRESETS: readonly McpClientPreset[] = [
  {
    id: 'claude-desktop',
    name: 'Claude Desktop',
    description: 'Anthropic 桌面端，stdio + HTTP 远程均支持',
    configPath: 'claude_desktop_config.json（设置 → 开发者 → 编辑配置）',
    rootKey: 'mcpServers',
    stdioType: 'stdio',
    httpType: 'http',
    supports: ['stdio', 'http'],
    source: 'Claude Code/Desktop 文档：`type: "http" | "stdio"`',
  },
  {
    id: 'claude-code',
    name: 'Claude Code',
    description: 'CLI / IDE 插件，项目级 .mcp.json 或用户级 ~/.claude.json',
    configPath: '.mcp.json（项目）或 ~/.claude.json（用户）',
    rootKey: 'mcpServers',
    stdioType: 'stdio',
    httpType: 'http',
    supports: ['stdio', 'http'],
    source: 'Claude Code 文档：`claude mcp add --transport http` 写出 type: "http"',
  },
  {
    id: 'cursor',
    name: 'Cursor',
    description: 'stdio 必须带 type；远程条目只写 url',
    configPath: '.cursor/mcp.json（项目）或 ~/.cursor/mcp.json（全局）',
    rootKey: 'mcpServers',
    // cursor.com/docs/mcp 字段表明确标注 stdio 的 type 为必填。
    stdioType: 'stdio',
    // 官方远程示例是裸 url，没有 type —— Cursor 从 url 推断传输方式。
    httpType: null,
    supports: ['stdio', 'http'],
    source: 'cursor.com/docs/mcp — STDIO 字段表 type=Yes；远程示例仅 url',
  },
  {
    id: 'vscode',
    name: 'VS Code (Copilot)',
    description: '根键是 servers 而非 mcpServers',
    configPath: '.vscode/mcp.json（工作区）或 命令面板 MCP: Open User Configuration',
    rootKey: 'servers',
    // 官方示例里 playwright 这类本地服务器不写 type。
    stdioType: null,
    httpType: 'http',
    supports: ['stdio', 'http'],
    source: 'code.visualstudio.com/docs/copilot/chat/mcp-servers — 顶层 servers',
  },
  {
    id: 'cline',
    name: 'Cline',
    description: '远程必须写 streamableHttp，写成 http 会退回旧版 SSE',
    configPath: 'Cline 面板 → MCP Servers → Configure，或 ~/.cline/mcp.json',
    rootKey: 'mcpServers',
    stdioType: null,
    // 注意大小写：Cline 认的是 streamableHttp，不是 http。
    httpType: 'streamableHttp',
    supports: ['stdio', 'http'],
    source: 'Cline 文档：`type: "streamableHttp"` + url，缺省会退回 SSE',
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    description: '配置结构与 Cursor 相同',
    configPath: '~/.codeium/windsurf/mcp_config.json',
    rootKey: 'mcpServers',
    stdioType: null,
    httpType: null,
    supports: ['stdio', 'http'],
    source: 'Windsurf 文档：JSON 结构同 Claude Desktop / Cursor',
  },
  {
    id: 'continue',
    name: 'Continue',
    description: 'stdio 形态可靠；远程配置各版本差异较大，暂不提供',
    configPath: 'config.json 的 mcpServers 段',
    rootKey: 'mcpServers',
    stdioType: null,
    httpType: null,
    supports: ['stdio'],
    source: 'Continue 文档 — 远程存在数组/对象两种历史形态，不做猜测',
  },
  {
    id: 'zed',
    name: 'Zed',
    description: '根键是 context_servers，且不识别 type 字段',
    configPath: 'settings.json 的 context_servers 段（zed: open settings file）',
    rootKey: 'context_servers',
    // Zed 从 command/url 推断传输方式，写 type 反而会被拒。
    stdioType: null,
    httpType: null,
    needsSourceField: true,
    supports: ['stdio', 'http'],
    source: 'zed.dev/docs/ai/mcp — context_servers + source:"custom"',
  },
  {
    id: 'http-generic',
    name: '通用 HTTP',
    description: '任何支持远程 MCP 的客户端，客户端无需安装二进制',
    configPath: '任意支持远程 MCP 端点的客户端配置',
    rootKey: 'mcpServers',
    stdioType: 'stdio',
    httpType: 'http',
    supports: ['http'],
    source: 'MCP 传输参考：{ "type": "http", "url": "https://.../mcp" }',
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

/**
 * URL the stdio bridge should dial. Note this is the daemon root, not
 * the `/mcp` path — `simple-post-mcp --url=` appends `/mcp` itself, so
 * handing it the full path would produce `/mcp/mcp`.
 */
export function bridgeTargetUrl(port: number | null): string | null {
  if (port === null || !Number.isInteger(port) || port <= 0) return null;
  return `http://127.0.0.1:${port}`;
}

/** One entry of a client's server map. */
export type ServerEntry = Record<string, unknown>;

/**
 * Build the single server entry for a preset + transport.
 *
 * Returns `null` when a required fact is missing — callers render an
 * explanatory placeholder rather than emitting a snippet with a literal
 * `null` in it, because a copyable-but-broken config is worse than an
 * honest "启动 MCP 后可用".
 */
export function buildServerEntry(
  preset: McpClientPreset,
  transport: McpTransport,
  facts: McpEndpointFacts,
): ServerEntry | null {
  const entry: ServerEntry = {};
  if (preset.needsSourceField) entry.source = 'custom';

  if (transport === 'http') {
    const url = daemonMcpUrl(facts.port);
    if (!url) return null;
    if (preset.httpType) entry.type = preset.httpType;
    entry.url = url;
    return entry;
  }

  // stdio — needs an absolute bridge path AND a port to dial.
  if (!facts.proxyPath) return null;
  const target = bridgeTargetUrl(facts.port);
  if (!target) return null;
  if (preset.stdioType) entry.type = preset.stdioType;
  entry.command = facts.proxyPath;
  entry.args = ['--url', target];
  if (preset.needsSourceField) entry.env = {};
  return entry;
}

/** Wrap a server entry under the client's own root key. */
export function wrapForClient(
  preset: McpClientPreset,
  entry: ServerEntry,
): Record<string, unknown> {
  return { [preset.rootKey]: { [MCP_SERVER_KEY]: entry } };
}

/**
 * Full, copy-pasteable snippet for one client + transport.
 *
 * Returns `null` when the daemon has no port yet or the bridge path has
 * not resolved — see {@link buildServerEntry}.
 */
export function buildClientConfig(
  preset: McpClientPreset,
  transport: McpTransport,
  facts: McpEndpointFacts,
): string | null {
  const entry = buildServerEntry(preset, transport, facts);
  if (!entry) return null;
  return JSON.stringify(wrapForClient(preset, entry), null, 2);
}

/**
 * Human-readable reason a snippet is unavailable, for the placeholder UI.
 * Returning a sentence beats an empty code block because the user learns
 * what action unblocks them.
 */
export function unavailableReason(
  preset: McpClientPreset,
  transport: McpTransport,
  facts: McpEndpointFacts,
): string {
  if (!preset.supports.includes(transport)) {
    return `该客户端未提供${transport === 'http' ? '远程 HTTP' : 'stdio'}接入方式`;
  }
  if (facts.port === null) {
    return 'MCP 未运行 — 启动后即可生成配置（两种方式都依赖 daemon 端口）';
  }
  if (transport === 'stdio' && !facts.proxyPath) {
    return '未解析到桥接二进制路径 — stdio 方式需要本机安装 simple-post-mcp';
  }
  return '暂不可用';
}
