/**
 * 搜索的生命周期 —— 防抖 + 作废标记。与 React 无关，故可以单独测。
 *
 * 搜索框每敲一个字就换一轮。上一轮可能已经把请求发出去了，它的回调若照写状态，就会把
 * 上一轮的结果盖到这一轮上——界面上看着是「搜的是 B，显示的却是 A 的结果」。故这里给
 * 每一轮一个编号，只有编号仍是最新的一轮才写状态。
 */

/** 定时器。测试里换成可控的假定时器，故不必真等防抖的那几百毫秒。 */
export interface NodeSearchTimers {
  /** 排一个延时回调，返回句柄。 */
  set: (fn: () => void, ms: number) => unknown;
  /** 撤掉一个延时回调。 */
  clear: (handle: unknown) => void;
}

/** `createNodeSearch` 的入参。 */
export interface CreateNodeSearchOptions<T> {
  /** 真正发一次搜索。 */
  run: (text: string) => Promise<T[]>;
  /** 停多久才发请求。默认 250 毫秒。 */
  delayMs?: number;
  /** 定时器。默认用 `setTimeout` / `clearTimeout`。 */
  timers?: NodeSearchTimers;
  /** 状态回调。 */
  onSearching?: (searching: boolean) => void;
  /** 状态回调。 */
  onResults?: (rows: T[]) => void;
  /** 状态回调。 */
  onError?: (error: unknown) => void;
}

/** 一个搜索控制器。 */
export interface NodeSearch {
  /** 输入变了：开新一轮，上一轮作废。空串表示清空。 */
  start: (text: string) => void;
  /** 作废当前这一轮，并撤掉待发的请求。不写任何状态。 */
  cancel: () => void;
}

const DEFAULT_TIMERS: NodeSearchTimers = {
  set: (fn, ms) => setTimeout(fn, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

/**
 * 造一个搜索控制器。
 *
 * 防抖由控制器自己管：`start` 每次都撤掉上一次待发的请求，故连续输入只在停下之后
 * 发一次。作废靠轮次编号：每一轮 `start` 都把编号加一，回调写状态前先核对自己仍是
 * 最新的一轮。
 */
export function createNodeSearch<T>(options: CreateNodeSearchOptions<T>): NodeSearch {
  const { run, delayMs = 250, timers = DEFAULT_TIMERS, onSearching, onResults, onError } = options;

  /** 当前轮的编号。每一轮加一；只有编号等于它的回调才写状态。 */
  let generation = 0;
  /** 待发的定时器句柄。没有待发请求时为 null。 */
  let timer: unknown = null;

  const clearTimer = () => {
    if (timer === null) return;
    timers.clear(timer);
    timer = null;
  };

  return {
    start: (text) => {
      const mine = ++generation;
      clearTimer();
      if (text === '') {
        onResults?.([]);
        onSearching?.(false);
        return;
      }
      onSearching?.(true);
      timer = timers.set(() => {
        timer = null;
        // 这一轮已经被新一轮顶掉了，请求不必再发。
        if (mine !== generation) return;
        void run(text).then(
          (rows) => {
            if (mine !== generation) return;
            onResults?.(rows);
            onSearching?.(false);
          },
          (error: unknown) => {
            if (mine !== generation) return;
            onError?.(error);
            onSearching?.(false);
          },
        );
      }, delayMs);
    },
    cancel: () => {
      generation += 1;
      clearTimer();
    },
  };
}
