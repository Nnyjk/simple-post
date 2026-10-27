import type { HttpMethod } from './request';

export interface ApiResponse {
  status: number;
  statusText: string;
  durationMs: number;
  sizeBytes: number;
  headers: Record<string, string>;
  body: string;
  /** JSON 响应解析结果，供美化视图用。 */
  bodyJson?: unknown;
  contentType: string;
  error?: string;
  /**
   * 请求上下文（可选，由发送方填）：`HttpTester.handleSend` 填 method 与 url，
   * 供响应历史行显示这次响应出自哪个请求。
   */
  method?: HttpMethod;
  url?: string;
}

/**
 * endpoint 发送历史的一行落库记录——`crates/simple-post-core/src/domain.rs`
 * 里 `ResponseHistoryEntry` 的渲染器镜像。
 *
 * 套一层而不是加宽 `ApiResponse`：后者是当次响应，直接交给查看器；
 * `id` / `endpointId` / `createdAt` / `bodyTruncated` 是**这行记录的**身份与出处。
 *
 * `bodyTruncated` 尤其只能放在这里：服务端写库时会裁剪超长 body 并连
 * `bodyJson` 一起丢，所以读回来的行未必等于调用方发的那次响应。
 */
export interface ResponseHistoryEntry {
  /** 行 id，服务端生成，每条唯一，当 React key 用。 */
  id: string;
  /** 来源 endpoint 的 `nodes.id`。 */
  endpointId: string;
  /** 实际落库的内容，可能是被裁剪后的形态。 */
  response: ApiResponse;
  /** 服务端裁剪了 `response.body`，`response.bodyJson` 一并丢弃。 */
  bodyTruncated: boolean;
  /** Unix 毫秒，由数据库打戳而非渲染器。 */
  createdAt: number;
}

export interface McpStatus {
  running: boolean;
  pid?: number;
  uptime?: number;
  requestsHandled: number;
  lastError?: string;
}

/**
 * Streamable-HTTP MCP 监听器的持久化配置，对应 Rust 的 `McpHttpConfig`
 * （`#[serde(rename_all = "camelCase")]`）。两处含义超出类型本身：
 *
 * - `port: 0` = 交给 OS 分配。不固化成具体数字：每次重启都变的端口不能
 *   写进 AI 客户端的配置文件（见 AGENTS.md「MCP client must not hardcode
 * a daemon port」）。
 * - `allowRemote` 是 `host` 之外的第二道闸：绑了非回环地址而不开它，
 *   监听就暴露到局域网，下面的白名单字段只在开启时才有意义。
 */
export interface McpHttpConfig {
  /** 监听地址；未开 `allowRemote` 时仅回环。 */
  host: string;
  /** `0` = 由 OS 分配临时端口。 */
  port: number;
  /** MCP 端点挂载路径，默认 `/mcp`。 */
  path: string;
  /** 是否对外提供服务，默认 `false`。 */
  allowRemote: boolean;
  /** 接受的 `Host` 头；`['*']` 关闭该检查。 */
  allowedHosts: string[];
  /** 接受的 `Origin` 头；`['*']` 关闭该检查。 */
  allowedOrigins: string[];
}
