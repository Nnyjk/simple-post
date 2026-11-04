/**
 * 生成设置页「连接」里展示的 MCP 客户端配置片段。
 *
 * - **对字段名较真**：不存在统一的「MCP 客户端配置 schema」，每个客户端都自创一套，而且
 *   差异是**静默**的——根键写错或 `type` 值写错不会报错，只是服务端永远不出现在该客户端的
 *   工具列表里。这是「复制这份配置」按钮最坏的失败方式。
 * - **只发布一个预设**，两条理由：
 *   1. stdio 那几列对本应用是死重量。桥接二进制（`simple-post-mcp`）不由这些客户端的
 *      安装器装上，生成的 `command` 路径还得用户手改；HTTP 在客户端机器上不需要任何东西，
 *      而这正是能跨重启一直可用的安排（见 AGENTS.md「MCP client must not hardcode a
 *      daemon port」）。
 *   2. 预设只有验证过才值得发。每个客户端都有自己那份写法（`streamableHttp`、
 *      `context_servers`、`servers`…），会静默过期；过期的行比没有更糟，因为用户分辨不出
 *      它已经过期。
 * - 已验证的 Claude Code HTTP 形状：根键 `mcpServers`，http 项写 `type: "http"` 加 `url`。
 * - 端点地址分两种形态算：桌面端连守护进程的本机端口；Web 端连服务端进程暴露出来的那个
 *   端点。Web 端**不能**拿守护进程的端口来拼——那个进程跑在服务端进程内部，端口由操作系统
 *   分配，也没有发布到宿主上，客户端照着拼出来的地址连过去只会连接被拒。
 * - 保持为纯模块（无 React、无 IPC），以便直接单测，面板里则不留业务逻辑。
 */

/**
 * 生成器需要的运行时事实。
 *
 * 两种部署形态各有各的 MCP 端点，故这里是一个可辨识联合而不是一堆可空字段：
 *
 * - **桌面端** —— MCP 由守护进程子进程应答，端口由操作系统分配。客户端连的是
 *   `http://127.0.0.1:<那个端口>/mcp`。
 * - **Web 端（容器）** —— MCP 由服务端进程自己应答：主端口上的 `/mcp`，以及配置了端口时
 *   那个只服务 MCP 的专用监听器。守护进程子进程跑在服务端进程内部，它的端口既不在客户端
 *   机器上、也没有被发布出去，故**不能**拿它拼客户端配置。
 */
export type McpEndpointFacts =
  | {
      /** 桌面端：守护进程。 */
      shape: 'desktop';
      /** 运行中守护进程的端口，来自 `mcp_status` 快照；停着时为 `null`。 */
      daemonPort: number | null;
    }
  | {
      /** Web 端：服务端进程。 */
      shape: 'web';
      /** 页面自己的 origin，如 `http://192.168.12.81:54173`。 */
      origin: string;
      /** 反向代理前缀（`/sp` 之类）；没有前缀时是空串。 */
      basePath: string;
      /** MCP 专用监听器的端口；`0` 表示没有专用端口，MCP 只走主端口。 */
      mcpPort: number;
      /** MCP 专用监听器的挂载路径。 */
      mcpPath: string;
    };

/**
 * 我们为其生成片段的一个 AI 客户端。
 *
 * `configPath` 是以散文展示的**提示**——本应用从不改第三方客户端的配置文件。
 * `source` 是该形状取自哪份文档，要留着：这些格式会变，下一个改动的人需要知道它是
 * 查证来的还是猜的。
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

/** 把 origin 的端口换成另一个端口；origin 解析不了时返回 `null`。 */
function originWithPort(origin: string, port: number): string | null {
  try {
    const url = new URL(origin);
    url.port = String(port);
    return url.origin;
  } catch {
    return null;
  }
}

/**
 * MCP 端点的完整地址。事实不足时返回 `null`，由界面说明为什么给不出地址。
 *
 * 两种形态各走各的：桌面端指向守护进程的本机端口；Web 端指向服务端进程暴露出来的那个
 * 端点——配了专用端口就用专用端口，没配就用主端口上的 `/mcp`。
 */
export function mcpEndpointUrl(facts: McpEndpointFacts): string | null {
  if (facts.shape === 'desktop') return daemonMcpUrl(facts.daemonPort);
  const base = facts.basePath.replace(/\/+$/, '');
  if (facts.mcpPort > 0) {
    const origin = originWithPort(facts.origin, facts.mcpPort);
    if (!origin) return null;
    const path = facts.mcpPath.startsWith('/') ? facts.mcpPath : `/${facts.mcpPath}`;
    return `${origin}${path}`;
  }
  return `${facts.origin}${base}/mcp`;
}

/** 客户端 server map 里的一个条目。 */
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
  const url = mcpEndpointUrl(facts);
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
  if (facts.shape === 'desktop' && facts.daemonPort === null) {
    return 'MCP 未运行 — 启动后即可生成配置';
  }
  if (facts.shape === 'web' && facts.origin === '') {
    return '无法确定本机的地址 — 请在浏览器里打开这个页面';
  }
  return '暂不可用';
}
