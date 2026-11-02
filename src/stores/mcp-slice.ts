/**
 * MCP 切片 —— 拥有共享 MCP 守护进程状态快照的轮询生命周期。
 *
 * 职责：
 *   - 持有 `mcpStatus`（首次成功轮询前为 null）。
 *   - 拥有**单例**的 3s `setInterval`，这样多个组件各自在 `useEffect` 里调
 *     `startMcpPolling()` 也不会把定时器叠起来。
 *   - 装一个 `document.visibilitychange` 监听器，标签页隐藏期间暂停定时器
 *     ——用户没东西可看，守护进程的状态也不会在唤醒之间变化。
 *   - 让消费方强制刷新（`refreshMcpStatus`）、重启（`restartMcp`），
 *     以及对自动弹出的"重启"对话框做去重（`restartDialogSeen`）。
 *
 * 本切片从 `StateCreator<AppState, ...>` 拿到 `(set, get, store)`，因此与其他
 * 切片共用同一个底层 Zustand store。它绝不直接 import 另一个切片模块——
 * 与 store 其余部分唯一的耦合是共享的 `AppState` 类型。
 */
import type { StateCreator } from 'zustand';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type {
  DaemonControlResult,
  McpLogTail,
  McpRuntimeStatus,
  McpUiState,
  RestartResult,
} from '@/lib/mcp-types';
import type { AppState } from './app-store';

/** 共享 MCP 守护进程快照的轮询周期。唯一来源。 */
const MCP_REFRESH_INTERVAL_MS = 3000;

// 仅浏览器——持有绑定的 `visibilitychange` handler，好让 store 在
// `stopMcpPolling` 时摘掉它。store 动作是唯一消费者，所以模块级变量
// （不进 React state）就够。
let _mcpVisibilityHandler: ((this: Document, ev: Event) => unknown) | null = null;
function _setVisibilityHandler(h: ((this: Document, ev: Event) => unknown) | null): void {
  _mcpVisibilityHandler = h;
}
function _getVisibilityHandler(): ((this: Document, ev: Event) => unknown) | null {
  return _mcpVisibilityHandler;
}

/**
 * MCP 守护进程"没在跑"的标准快照。`restartMcp` 里每个新建 `McpUiState`
 * 的默认分支（因为轮询快照还没返回）都展开这个常量再覆盖 `status`
 * （失败路径上还覆盖 `lastError`）。把字段表收在一处，新增 `McpUiState`
 * 字段时只需改这个常量加上 `@/lib/mcp-types` 里的类型声明，
 * 而不是在下面散落的三个动作体里各改一次。
 */
const EMPTY_MCP_STATE: McpUiState = {
  status: 'stopped',
  running: false,
  pid: null,
  port: null,
  uptimeMs: 0,
  requestsHandled: 0,
  currentConnections: 0,
  registeredTools: [],
};

/** MCP 切片拥有的 state。 */
export interface McpSliceState {
  /**
   * 共享的 MCP 守护进程快照。无论多少组件消费，都由**唯一一个** `setInterval`
   * 每 3s 轮询一次。首次成功轮询之前为 `null`（渲染方据此渲染加载态）。
   */
  mcpStatus: McpUiState | null;
  /** 仅浏览器。当前的 3s 定时器 id，空闲时为 `null`。 */
  _mcpTimer: number | null;
  /**
   * ConnectionSection 自动弹出的重启对话框**已经**为哪个 `status` 触发过。
   * 用于去重——守护进程连续两次报 `crashed`（或 `unhealthy`）时，对话框应该
   * 出现一次而不是两次。放在 store 上，自动弹出那个 effect 才能熬过设置标签的
   * 反复挂载。
   */
  restartDialogSeen: McpRuntimeStatus | null;
}

/** MCP 切片拥有的动作。 */
export interface McpSliceActions {
  /**
   * 若 3s 轮询循环尚未运行就开始它。幂等——N 个组件都调用也只会得到**一个**
   * `setInterval` 和每 tick 一次 IPC。
   *
   * 同时装一个 `document.visibilitychange` 监听器，在标签页隐藏期间暂停定时器
   * （看不见徽标的用户没必要轮询）。监听器由 `stopMcpPolling` 摘掉。
   */
  startMcpPolling: () => void;
  /**
   * 停掉轮询循环并摘下可见性监听器。每次 `startMcpPolling`（通常来自
   * `useMcpStatus` 的挂载）都要配一个 `stopMcpPolling` 清理，否则会泄漏。
   */
  stopMcpPolling: () => void;
  /**
   * 强制向守护进程刷新一次并把结果推入 `mcpStatus`。失败时只记日志（让 UI 继续
   * 渲染上一次的好状态），不把 `mcpStatus` 覆盖成"已停止"的快照——那会用一次
   * 瞬时的 IPC 失败掩盖真实的守护进程崩溃。
   */
  refreshMcpStatus: () => Promise<void>;
  /**
   * 触发守护进程重启命令。在 **UI** 层幂等（组件才是调用方，重启在途时按钮
   * 会被禁用）。失败时翻 `mcpStatus.lastError`，好让崩溃对话框显示底层原因。
   */
  restartMcp: () => Promise<void>;
  /**
   * 手动运行时开关——守护进程没在跑就启动它。不同于 `mcp.auto_start`
   * （启动期偏好），也不同于 `restartMcp`（后者总是先把旧进程拆掉）。
   *
   * 该命令在服务端幂等：守护进程健康时调用会直接返回它现有的
   * `{ok, pid, port}` 而不重新 spawn，所以绝不会因此让用户丢掉连接。
   */
  startMcp: () => Promise<void>;
  /**
   * 手动运行时关——终止守护进程并释放它的端口文件。什么都没跑时调用是安全的
   * （`ok: true, pid: null`）。
   *
   * 这刻意**不是** `mcp.auto_start = false`：auto_start 只决定应用启动时守护
   * 进程是否起来，关掉它绝不能杀掉用户正在用的守护进程。
   */
  stopMcp: () => Promise<void>;
  /**
   * 记录自动弹出的重启对话框已为给定 status 展示过。幂等——ConnectionSection
   * effect 的去重逻辑在用户关掉对话框时调它，好让同一 status 的下一次出现
   * 不会立刻重新弹出来。
   */
  markRestartDialogSeen: (status: McpRuntimeStatus) => void;
  /**
   * 把守护进程最近报告的错误记到 `mcpStatus.lastError`。McpStatusPanel 里的
   * `daemon://crashed` 事件监听器用它立刻显示守护进程给的错误串，而不用等下
   * 一次 3s 轮询返回。传 `null` 清空该字段（例如成功重启之后）。尚无
   * `mcpStatus` 快照时是空操作——面板能容忍这段加载空档。把它收在这里，是为了让
   * 渲染层不必直接对 `mcpStatus` 调 `setState`，那会绕过本切片文档承诺的每一条
   * 不变量。
   */
  recordLastError: (lastError: string | null) => void;
  /**
   * 读已落盘的 `mcp.auto_start` 偏好。首次读取返回前为 `null`；面板在这段窗口
   * 里渲染禁用的开关，用户因此不会看到还没读到的值被显示成"关"。
   */
  mcpAutoStart: boolean | null;
  /**
   * daemon 最近若干行日志。首次拉取前为 `null`。
   *
   * **刻意不并进 3s 轮询**：状态快照很小，日志可能有几百行，每 3s 拉一次是白花的
   * 带宽。日志区展开时才拉。
   */
  daemonLogs: McpLogTail | null;
  /**
   * 重新拉一次 daemon 日志。失败时保留上一次的好内容——瞬时的 IPC 失败不该把用户
   * 正在读的日志清空。
   */
  refreshDaemonLogs: () => Promise<void>;
  /**
   * 落盘 `mcp.auto_start`。用户打开开关时顺便让守护进程启动（幂等），省得再点
   * 一次；关掉时对正在运行的守护进程不动——auto_start 管的是启动时起不起，
   * 不是运行时杀进程的开关。
   */
  setMcpAutoStart: (value: boolean) => Promise<void>;
  /**
   * 从后端重读 `mcp.auto_start` 偏好。ConnectionSection 挂载时调用，好让开关
   * 反映已落盘的值（该行不存在时默认 `false`）。
   */
  refreshMcpAutoStart: () => Promise<void>;
}

export type McpSlice = McpSliceState & McpSliceActions;

/**
 * 生命周期动词（start / stop / restart）回答的都是同一种
 * `{ ok, pid, port }` 信封，所以三者的徽标记账完全一样。这两个助手就是为了
 * 避免每个动作各推一遍。
 *
 * `set` 是传进来的而非闭包捕获的，因为它们位于模块作用域；工厂捕获 `set` 再往下传。
 */
type SetState = (partial: Partial<AppState>) => void;

/**
 * 把 `mcpStatus.status` 强设为 `status`，其余字段保留上一次已知快照。首次轮询
 * 返回前退回到 `EMPTY_MCP_STATE`，好让徽标绝不会从残缺对象渲染。
 */
function withStatus(get: () => AppState, status: McpRuntimeStatus): McpUiState {
  const cur = get().mcpStatus;
  return cur ? { ...cur, status } : { ...EMPTY_MCP_STATE, status };
}

/**
 * `ok: false` **不是**异常——Rust 命令会吞掉底层 `AppError` 并在信封里报告它
 * ——所以它需要和异常路径一样的"翻徽标、让对话框去解释"处理。每个生命周期动词
 * 都把失败的调用落定在 `crashed`。
 */
function paintFailure(get: () => AppState, set: SetState): void {
  set({ mcpStatus: withStatus(get, 'crashed') });
}

export const createMcpSlice: StateCreator<AppState, [], [], McpSlice> = (set, get) => ({
  // MCP 守护进程 state —— 由 `startMcpPolling()` 懒填充。初始的 `null` 让消费方
  // 在第一次 `refreshMcpStatus()` 往返完成之前渲染"加载中"。
  mcpStatus: null,
  _mcpTimer: null,
  restartDialogSeen: null,
  /**
   * 已落盘的 `mcp.auto_start` 偏好。由 `ConnectionSection` 的 effect 通过
   * `refreshMcpAutoStart` 填充；首次 IPC 之前为 null。服务端默认 `false`，
   * 所以还没碰过开关的用户读一次之后看到的就是 `false`。
   */
  mcpAutoStart: null,
  /**
   * daemon 日志。首次 `refreshDaemonLogs` 之前为 null。
   */
  daemonLogs: null,
  refreshDaemonLogs: async () => {
    try {
      const dto = await invokeCmd<McpLogTail>(COMMANDS.mcpDaemonLogTail);
      set({ daemonLogs: dto });
    } catch (e) {
      // 失败时保留上一次的好内容：瞬时的 IPC 失败不该把用户正在读的日志清空。
      console.warn('[store] mcp_daemon_log_tail failed', e);
    }
  },

  // ---------- MCP 守护进程轮询 ----------
  // 单例 3s tick 有三个协作方：
  //   - `useMcpStatus` 在挂载 / 清理时调 start() / stop()
  //   - `McpStatusPanel` 拥有 `daemon://crashed` 事件监听器，
  //     崩溃时通过 `refreshMcpStatus` 强制刷新。
  //   - `ConnectionSection` 在手动重启后调 `refreshMcpStatus`，
  //     让徽标立刻翻回 `running`。
  startMcpPolling: () => {
    const state = get();
    if (state._mcpTimer !== null) return; // idempotent — only one timer ever
    const tick = () => {
      void state.refreshMcpStatus();
    };
    const timer = window.setInterval(tick, MCP_REFRESH_INTERVAL_MS);
    set({ _mcpTimer: timer });
    // 立刻发起第一次读取，面板挂载后 `mcpStatus: null` 最多只维持几毫秒而不是 3s。
    tick();

    // 标签页隐藏期间暂停——没东西可显示，守护进程也不会在唤醒之间变化。
    // 监听器由 store 持有（模块级 handler 引用），所以每次 start() 都不会
    // 新增一个，stop() 也能干净地摘掉。
    if (typeof document !== 'undefined' && _getVisibilityHandler() === null) {
      const handler = () => {
        if (document.visibilityState === 'hidden') {
          // 暂停：清掉定时器但保留 handler 引用，好让 start() 能从
          // visibilitychange→'visible' 分支恢复。
          const cur = get();
          if (cur._mcpTimer !== null) {
            window.clearInterval(cur._mcpTimer);
            set({ _mcpTimer: null });
          }
        } else if (document.visibilityState === 'visible') {
          // 恢复——start() 幂等，可以放心直接调。
          get().startMcpPolling();
        }
      };
      document.addEventListener('visibilitychange', handler);
      _setVisibilityHandler(handler);
    }
  },
  stopMcpPolling: () => {
    const state = get();
    if (state._mcpTimer !== null) {
      window.clearInterval(state._mcpTimer);
    }
    // 摘掉可见性监听器——本该只由最后一个卸载的消费方拆掉，但它幂等，
    // 所以在这里调是安全的。
    if (typeof document !== 'undefined') {
      const handler = _getVisibilityHandler();
      if (handler) {
        document.removeEventListener('visibilitychange', handler);
        _setVisibilityHandler(null);
      }
    }
    set({ _mcpTimer: null });
  },
  refreshMcpStatus: async () => {
    try {
      // Rust 的 `McpStatusDto` 带 `#[serde(rename_all =
      // "camelCase")]` 以及新增的 `status` 与 `current_connections` 字段，
      // 所以线路形状现在与 `McpUiState` 一一对应。这一个强制转换是唯一的
      // 逃生口——渲染层信任守护进程给的生命周期字符串，而徽标对任何未知值都
      // 退回 `'stopped'`，所以运行时不匹配不可能以 TS 错误的形式暴露出来。
      const dto = (await invokeCmd<unknown>(COMMANDS.mcpStatus)) as McpUiState;
      set({ mcpStatus: dto });
    } catch (e) {
      // 守护进程不可达（冷启动、没有守护进程监管器的开发模式，或 IPC 竞态）
      // ——记日志并保留上一次的好状态。刻意**不**把 `mcpStatus` 覆盖成"已停止"的
      // 快照，那会用瞬时的 IPC 失败掩盖真实的守护进程崩溃。
      console.warn('[store] mcp_status poll failed', e);
    }
  },
  restartMcp: async () => {
    // 渲染层在这里的职责是：立刻把徽标翻成"重启中"，发出命令，然后刷新让新的
    // PID / status 可见。UI 按钮的 `disabled` 状态是调用方的活（它用局部 state
    // 跟踪在途，因为往返可能要 100ms–2s，用户一点就该立刻禁用）。
    const prev = get().mcpStatus;
    set({
      mcpStatus: prev
        ? { ...prev, status: 'restarting' }
        : { ...EMPTY_MCP_STATE, status: 'restarting' },
    });
    try {
      const result: RestartResult = await invokeCmd<RestartResult>(COMMANDS.mcpDaemonRestart);
      if (!result.ok) {
        // Rust 吞掉了底层 `AppError` 并返回 `{ ok: false, pid: null, port: null }`。
        // 这里摆出与异常路径相同的 `crashed` 徽标，好让对话框接得住；Rust 没给错误串，
        // 所以 `lastError` 留空，交给下一次轮询去显示守护进程自己报告的东西。
        console.warn('[store] mcp_daemon_restart returned ok=false');
        const cur = get().mcpStatus;
        set({
          mcpStatus: cur
            ? { ...cur, status: 'crashed' }
            : { ...EMPTY_MCP_STATE, status: 'crashed' },
        });
        return;
      }
      await get().refreshMcpStatus();
    } catch (e) {
      console.error('[store] mcp_daemon_restart failed', e);
      // 这里把失败摆到徽标上——对话框会读 lastError。
      const cur = get().mcpStatus;
      const lastError = e instanceof Error ? e.message : String(e);
      set({
        mcpStatus: cur
          ? { ...cur, status: 'crashed', lastError }
          : { ...EMPTY_MCP_STATE, status: 'crashed', lastError },
      });
      throw e;
    }
  },
  /** 手动开。契约上幂等：Rust 命令在守护进程健康时直接返回现有那个而不重新 spawn，
   *  所以连点两下不会让用户付出任何代价。
   */
  startMcp: async () => {
    const prev = get().mcpStatus;
    set({
      mcpStatus: prev
        ? { ...prev, status: 'starting' }
        : { ...EMPTY_MCP_STATE, status: 'starting' },
    });
    try {
      const result = await invokeCmd<DaemonControlResult>(COMMANDS.mcpDaemonStart);
      if (!result.ok) {
        console.warn('[store] mcp_daemon_start returned ok=false');
        paintFailure(get, set);
        return;
      }
      await get().refreshMcpStatus();
    } catch (e) {
      console.error('[store] mcp_daemon_start failed', e);
      const cur = get().mcpStatus;
      const lastError = e instanceof Error ? e.message : String(e);
      set({
        mcpStatus: cur
          ? { ...cur, status: 'crashed', lastError }
          : { ...EMPTY_MCP_STATE, status: 'crashed', lastError },
      });
      throw e;
    }
  },
  /** 手动关。刻意独立于 `mcp.auto_start`——见接口上的说明。 */
  stopMcp: async () => {
    try {
      const result = await invokeCmd<DaemonControlResult>(COMMANDS.mcpDaemonStop);
      if (!result.ok) {
        console.warn('[store] mcp_daemon_stop returned ok=false');
      }
      // 无论如何都落定到真实上报的状态。`stopped` 是预期结果；若服务端不同意
      // （例如 healthz 竞态让它还活着），由轮询纠正我们，而不是我们猜。
      await get().refreshMcpStatus();
      const cur = get().mcpStatus;
      if (!cur || cur.status === 'running' || cur.status === 'starting') {
        set({ mcpStatus: withStatus(get, 'stopped') });
      }
    } catch (e) {
      console.error('[store] mcp_daemon_stop failed', e);
      throw e;
    }
  },
  markRestartDialogSeen: (status) => {
    set({ restartDialogSeen: status });
  },
  recordLastError: (lastError) => {
    set((s) => {
      if (!s.mcpStatus) return {};
      // `McpUiState.lastError` 是可选的（`string | undefined`）而不是
      // `string | null`。把面板监听器传来的显式 `null`"清空"哨兵转成
      // `undefined`，展开后类型才能在严格模式下通过。
      const next: string | undefined = lastError ?? undefined;
      if (s.mcpStatus.lastError === next) return {};
      return { mcpStatus: { ...s.mcpStatus, lastError: next } };
    });
  },
  /**
   * 读已落盘的自动启动偏好。失败静默——连接面板能容忍该行读不到，
   * 办法是让开关留在未定的 `null` 状态。
   */
  refreshMcpAutoStart: async () => {
    try {
      const v = await invokeCmd<boolean>(COMMANDS.mcpAutoStartGet);
      set({ mcpAutoStart: v });
    } catch (e) {
      console.warn('[store] mcp_auto_start_get failed', e);
    }
  },
  setMcpAutoStart: async (value) => {
    // 乐观翻转——面板在往返期间禁用开关，开关要立刻反映用户意图。
    // setter 失败时抛错，好让面板把开关滚回去并显示错误。
    set({ mcpAutoStart: value });
    try {
      await invokeCmd<boolean>(COMMANDS.mcpAutoStartSet, { value });
      // 刚打开自动启动的话守护进程可能已经起来了——这里刷新快照，
      // 不必等下一个 3s tick 徽标才更新。
      if (value) {
        await get().refreshMcpStatus();
      }
    } catch (e) {
      // 回滚并重抛，好让面板弹 toast。
      set({ mcpAutoStart: !value });
      throw e;
    }
  },
});
