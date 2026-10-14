/**
 * 生成设置页「连接」里展示的 MCP 客户端配置片段。
 *
 * ## 为什么对字段名这么较真
 *
 * 不存在统一的「MCP 客户端配置 schema」，每个客户端都自创一套，而且差异是**静默**
 * 的——根键写错或 `type` 值写错不会报错，只是服务端永远不出现在该客户端的工具列表
 * 里。这是「复制这份配置」按钮最坏的失败方式。
 *
 * ## 为什么只有一个预设
 *
 * 当前表里只有 Claude Code 的 HTTP 一项。再加行之前请记住两条理由：
 *
 * 1. **stdio 那几列对本应用是死重量。** 桥接二进制（`simple-post-mcp`）不由这些
 *    客户端的安装器装上，生成的 `command` 路径还得用户手改。HTTP 在客户端机器上
 *    不需要任何东西，而这也正是能跨重启一直可用的安排（见 AGENTS.md「MCP client
 *    must not hardcode a daemon port」）。
 * 2. **预设只有验证过才值得发。** 每个客户端都有自己那份写法（`streamableHttp`、
 *    `context_servers`、`servers`…），会静默过期。过期的行比没有更糟，因为用户分辨
 *    不出它已经过期。
 *
 * 已验证的 Claude Code HTTP 形状，原样保留：
 *
 * | Client      | root key     | http                                 |
 * |-------------|--------------|--------------------------------------|
 * | Claude Code | `mcpServers` | `type: "http"` + `url`               |
 *
 * 保持为纯模块（无 React、无 IPC），以便直接单测，面板里则不留业务逻辑。
 */

/**
 * 生成器需要的运行时事实。可空——daemon 报出端口之前，面板也得渲染出有用的东西。
 */
export interface McpEndpointFacts {
  /** 运行中 daemon 的端口，来自 `mcp_status` 快照。 */
  port: number | null;
}

/**
 * 本模块生成的唯一传输方式。
 *
 * 保留为具名类型而非删掉：它仍是面板渲染的契约，也是日后新增传输方式时唯一要动
 * 的地方。
 */
export type McpTransport = 'http';

/**
 * 我们为其生成片段的一个 AI 客户端。
 *
 * `configPath` 是以散文展示的**提示**——本应用从不改第三方客户端的配置文件。
 * `source` 是该形状取自哪份文档，要留着：这些格式会漂移，下一个改动的人需要知道
 * 它是查证来的还是猜的。
 */
export interface McpClientPreset {
  id: string;
  description: string;
  /** 用户粘贴的位置。散文，可以提 OS 差异。 */
  configPath: string;
  /** 客户端读取 servers 的顶层键。 */
  rootKey: 'mcpServers';
  /** 放进 `type` 的值；Claude Code 的 HTTP 项要求它。 */
  httpType: string;
  /** 该形状在哪儿验证过。 */
  source: string;
}

/**
 * 本应用发布的唯一预设。
 *
 * 用数组而非裸对象，好让面板继续渲染列表，也让
 * `MCP_CLIENT_PRESETS.length === 1` 本身成为测试对「只发布一种格式」的断言。
 */
export const MCP_CLIENT_PRESETS: readonly McpClientPreset[] = [
  {
    id: 'claude-code',
    description: 'CLI / IDE 插件，项目级 .mcp.json 或用户级 ~/.claude.json',
    configPath: '.mcp.json（项目）或 ~/.claude.json（用户）',
    rootKey: 'mcpServers',
    // Claude Code 自己用 `claude mcp add --transport http` 写出这个字段；
    // 远程项不带 `type` 就不是该 CLI 产出的形状
    httpType: 'http',
    source: 'Claude Code 文档：`claude mcp add --transport http` 写出 type: "http"',
  },
] as const;

/** 每段生成的片段里使用的 server key。 */
export const MCP_SERVER_KEY = 'simple-post';

/**
 * 运行中 daemon 的 MCP 端点 base URL；daemon 还没报出端口（停了 / 崩了）
 * 时为 `null`。
 */
export function daemonMcpUrl(port: number | null): string | null {
  if (port === null || !Number.isInteger(port) || port <= 0) return null;
  return `http://127.0.0.1:${port}/mcp`;
}

/** One entry of a client's server map. */
export type ServerEntry = Record<string, unknown>;

/**
 * 为一个预设构造那唯一的 server 条目。
 *
 * 缺少必需事实时返回 `null`，调用方渲染解释性占位，而不是发出一个里面带着字面量
 * `null` 的片段——能复制但坏掉的配置比一句诚实的「启动 MCP 后可用」更糟。
 */
export function buildServerEntry(
  preset: McpClientPreset,
  facts: McpEndpointFacts,
): ServerEntry | null {
  const url = daemonMcpUrl(facts.port);
  if (!url) return null;
  return { type: preset.httpType, url };
}

/** 把 server 条目裹进客户端自己的顶层键里。 */
export function wrapForClient(
  preset: McpClientPreset,
  entry: ServerEntry,
): Record<string, unknown> {
  return { [preset.rootKey]: { [MCP_SERVER_KEY]: entry } };
}

/**
 * 单个客户端的完整可粘贴片段。daemon 还没有端口时返回 `null`——
 * 见 {@link buildServerEntry}。
 */
export function buildClientConfig(preset: McpClientPreset, facts: McpEndpointFacts): string | null {
  const entry = buildServerEntry(preset, facts);
  if (!entry) return null;
  return JSON.stringify(wrapForClient(preset, entry), null, 2);
}

/**
 * 片段不可用的可读原因，供占位 UI 使用。返回一句话好过空代码块——用户由此知道
 * 做什么能解开。
 */
export function unavailableReason(preset: McpClientPreset, facts: McpEndpointFacts): string {
  void preset;
  if (facts.port === null) return 'MCP 未运行 — 启动后即可生成配置';
  return '暂不可用';
}
