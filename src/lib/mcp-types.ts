/**
 * 面向渲染器的 MCP daemon 状态类型。放在 `src/lib/`（纯 `.ts`），让 store、
 * 面板组件与 `useMcpStatus` hook 都能 import 同一个规范类型，而不必把 `.tsx`
 * 拖进 `stores/` 或经面板产生循环依赖。
 *
 * 形状镜像 Rust 的 `McpStatusDto`（见 `src-tauri/src/commands/`）。渲染器一侧
 * 容忍不匹配：未知的生命周期字符串在徽标里收敛为 `'stopped'`，绝不抛。
 */

/**
 * MCP daemon 的生命周期状态，由 `mcp_status` 驱动。未知值回落到 `'stopped'`。
 */
export type McpRuntimeStatus =
  'stopped' | 'starting' | 'running' | 'restarting' | 'crashed' | 'unhealthy';

/**
 * `mcp_status` 的 UI 投影。渲染器关心的字段：
 *
 *   - `status` —— 驱动徽标与崩溃对话框的触发
 *   - `running` —— 派生的布尔量，为仍在读它的老消费方保留；等于
 *     `status === 'running'`
 *   - `pid` / `port` —— 仅用于展示
 *   - `uptimeMs` —— daemon 已运行多久
 *   - `requestsHandled` —— 累计计数
 *   - `currentConnections` —— stdio / streamable-http 的汇入数
 *   - `registeredTools` —— MCP server 已宣告的工具；取代 ConnectionSection 里
 *     硬编码的 `REGISTERED_TOOLS` 常量
 *   - `lastError` —— 展示在崩溃对话框的描述里
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
 * `mcp_daemon_start` / `mcp_daemon_stop` / `mcp_daemon_restart` 三个命令返回的
 * 线格式。三个生命周期动词刻意共用**一个**信封：Rust 命令吞掉底层 `AppError`
 * 并报统一结果，于是渲染器保持一处可辨识分支，而不必在每个调用点写 try/catch
 * （或面对三种形状）。
 *
 *   - `ok: true` 且带 `pid`/`port` → 那个 daemon 是活的那个
 *   - `ok: true` 且 `pid: null`   → 一次合法的停止；什么都没在跑
 *   - `ok: false`                 → 拉起失败 / healthz 超时
 *
 * 成功启动后的 `pid` / `port` 是**新** daemon 的身份；对已在运行的 daemon 做幂等
 * 启动时，它们是**既有** daemon 的——那种情况服务端不会重新拉起。
 */
export interface RestartResult {
  ok: boolean;
  pid?: number | null;
  port?: number | null;
}

/**
 * 同一个信封，换个在 start/stop 调用点读起来更顺的名字。保留为别名而非第二个
 * 接口，两个方向的结构类型完全相同。
 */
export type DaemonControlResult = RestartResult;
