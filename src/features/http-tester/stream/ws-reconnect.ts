/**
 * WebSocket 自动重连的退避表与调度器。
 *
 * 连接掉线后按 `1s → 2s → 4s → 8s → 16s → 30s` 退避重试，最多 5 次；30s 是上限，只在把
 * `WS_MAX_RETRIES` 调大时才用得到。每次真正连上（收到 `open`）都清零计数，用户手动断开后
 * 不再重试。
 *
 * 时钟与定时器由调用方注入：面板要显示「还有几秒重连」的倒计时，测试要用假时钟驱动整条
 * 序列，两者都不该让本模块读 `Date.now()`。
 */

/** 第 n 次重试前的等待（毫秒）。 */
export const WS_BACKOFF_MS: readonly number[] = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];

/** 连续失败这么多次之后不再重试。 */
export const WS_MAX_RETRIES = 5;

export interface ReconnectTimers {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export interface ReconnectDeps {
  timers: ReconnectTimers;
  /** 墙上时间；`onSchedule` 报出的重连时刻由它算出。 */
  now: () => number;
  /** 重连到点后调用；由调用方发起一次连接。 */
  connect: () => void;
  /**
   * 待触发的重试变了（安排 / 触发 / 取消）时调用，`nextAt === null` 表示没有待触发的重试。
   * 面板用它在连接按钮旁画倒计时。
   */
  onSchedule: (nextAt: number | null, attempt: number) => void;
}

export interface ReconnectController {
  /** 连上了：失败计数与待触发的重试一起清掉。 */
  onOpen: () => void;
  /** 掉线了；`true` = 已安排下一次重试。 */
  onClosed: () => boolean;
  /** 关掉开关或用户断开：取消待触发的重试并把次数清零，之后再打开开关从头算。 */
  cancel: () => void;
  /** 关标签 / 切协议：取消待触发的重试，此后 `onClosed` 不再安排。 */
  dispose: () => void;
  /** 重试次数已用完。 */
  isExhausted: () => boolean;
}

/**
 * 建一个重连调度器。它只认「连上」与「掉了」两个事实，不知道连接是怎么建的——那个回调由
 * 调用方给。
 */
export function createReconnect(deps: ReconnectDeps): ReconnectController {
  let failures = 0;
  let attempt = 0;
  let timer: unknown = null;
  let nextAt: number | null = null;
  let disposed = false;

  const clearPending = () => {
    if (timer !== null) deps.timers.clearTimeout(timer);
    timer = null;
    if (nextAt !== null) {
      nextAt = null;
      deps.onSchedule(null, attempt);
    }
  };

  return {
    onOpen: () => {
      failures = 0;
      attempt = 0;
      clearPending();
    },
    onClosed: () => {
      if (disposed || timer !== null || failures >= WS_MAX_RETRIES) return false;
      const delay = WS_BACKOFF_MS[Math.min(failures, WS_BACKOFF_MS.length - 1)];
      failures += 1;
      attempt = failures;
      nextAt = deps.now() + delay;
      deps.onSchedule(nextAt, attempt);
      timer = deps.timers.setTimeout(() => {
        timer = null;
        nextAt = null;
        deps.onSchedule(null, attempt);
        deps.connect();
      }, delay);
      return true;
    },
    cancel: () => {
      failures = 0;
      attempt = 0;
      clearPending();
    },
    dispose: () => {
      disposed = true;
      clearPending();
    },
    isExhausted: () => failures >= WS_MAX_RETRIES,
  };
}
