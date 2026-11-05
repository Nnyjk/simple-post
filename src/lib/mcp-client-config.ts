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
 * - 端点地址只有一处：**MCP 守护进程自己的监听端口**。主端口不提供 MCP，故地址不再有
 *   「主端口上的 /mcp」那种回退。
 * - 保持为纯模块（无 React、无 IPC），以便直接单测，面板里则不留业务逻辑。
 */

import { isLoopbackHost } from './loopback-host';

/**
 * 生成器需要的运行时事实。
 *
 * 两种部署形态各有各的 MCP 端点，故这里是一个可辨识联合而不是一堆可空字段：
 *
 * - **桌面端** —— MCP 由守护进程子进程应答，客户端连的是 `http://127.0.0.1:<端口><路径>`。
 *   端口在监听配置里写死时就是那个写死的端口；写 `0`（由操作系统分配）时用运行中守护进程
 *   报出来的那个。
 * - **Web 端（容器）** —— MCP 同样由守护进程应答，但那个进程跑在服务端进程内部，它的端口
 *   要由部署侧发布出来才能被外部客户端连到。故客户端连的是「页面这台机器 + 监听配置里那个
 *   端口」。监听地址是回环地址时那个端口只在本机可达，外部客户端连不上——那种情况下
 *   给不出地址。
 */
export type McpEndpointFacts =
  | {
      /** 桌面端：守护进程。 */
      shape: 'desktop';
      /** 监听配置里的端口；`0` 表示由操作系统分配。 */
      configPort: number;
      /** 运行中守护进程报的端口；守护进程停着时为 `null`。 */
      daemonPort: number | null;
      /** 监听配置里的挂载路径。 */
      path: string;
    }
  | {
      /** Web 端：服务端进程里的守护进程。 */
      shape: 'web';
      /** 页面自己的 origin，如 `http://192.168.12.81:54173`。 */
      origin: string;
      /** 监听配置里的端口；`0` 表示没有固定端口，外部客户端无从知道该连哪一个。 */
      configPort: number;
      /** 监听配置里的绑定地址。 */
      host: string;
      /** 监听配置里的挂载路径。 */
      path: string;
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
 * 把挂载路径归一成 URL 片段：补上前导斜杠、去掉尾斜杠。
 *
 * 路径为空或就是 `/` 时返回空串——那种情况下 MCP 挂在根上，地址后面不再跟一段路径。
 */
export function mountPath(path: string): string {
  const trimmed = path.trim();
  if (trimmed === '' || trimmed === '/') return '';
  const withSlash = trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
  return withSlash.replace(/\/+$/, '');
}

/**
 * 运行中 daemon 的 MCP 端点 base URL；daemon 还没报出端口（停了 / 崩了）
 * 时为 `null`。
 */
export function daemonMcpUrl(port: number | null, path = '/mcp'): string | null {
  if (port === null || !Number.isInteger(port) || port <= 0) return null;
  return `http://127.0.0.1:${port}${mountPath(path)}`;
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
 * 两种形态各走各的：
 *
 * - 桌面端连本机的守护进程端口；监听配置里写死了端口就用那个写死的，写 `0` 时用运行中
 *   守护进程报出来的那个。
 * - Web 端连「页面这台机器 + 监听配置里那个端口」。那个进程在服务端进程内部，故外部
 *   客户端连不上这两种情况都给不出地址：端口没写死（客户端无从知道连哪一个），或者
 *   监听地址是回环地址（那个端口只在本机可达）。
 */
export function mcpEndpointUrl(facts: McpEndpointFacts): string | null {
  if (facts.shape === 'desktop') {
    const port = facts.configPort > 0 ? facts.configPort : facts.daemonPort;
    return daemonMcpUrl(port, facts.path);
  }
  if (facts.configPort <= 0) return null;
  if (isLoopbackHost(facts.host)) return null;
  const origin = originWithPort(facts.origin, facts.configPort);
  if (!origin) return null;
  return `${origin}${mountPath(facts.path)}`;
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
 * 单个客户端的完整可粘贴片段。端点地址给不出来时返回 `null`——
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
export function unavailableReason(facts: McpEndpointFacts): string {
  if (facts.shape === 'desktop') {
    if (facts.configPort > 0) return '暂不可用';
    if (facts.daemonPort === null) return 'MCP 未运行 — 启动后即可生成配置';
    return '暂不可用';
  }
  if (facts.origin === '') return '无法确定本机的地址 — 请在浏览器里打开这个页面';
  if (facts.configPort <= 0) return '监听配置里没有写死端口 — 客户端无从知道该连哪一个';
  if (isLoopbackHost(facts.host)) {
    return '监听地址是回环地址 — 外部客户端连不上，把它改成 0.0.0.0 才能对外提供';
  }
  return '暂不可用';
}
