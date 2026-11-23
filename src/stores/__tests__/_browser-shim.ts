/**
 * Node 环境下的浏览器与 Tauri IPC 模拟桩。
 *
 * `src/stores/*.ts` 面向 Tauri webview 运行，直接引用 `window.setInterval`、
 * `document.visibilityState`、`window.localStorage` 与 `window.__TAURI_INTERNALS__.invoke`。
 * 本模块补齐切片运行所需的浏览器 API，并基于 `@tauri-apps/api/mocks#mockIPC` 桩化 Tauri IPC。
 * 导出：
 *   - `installBrowserShim()`：挂载 `window`、`document`、`localStorage`，提供 `visibilitychange` 调度；
 *   - `installIpcMock(handler)`：拦截 `invoke`，handler 抛错触发切片 catch；
 *   - `clearIpcMock()`：重置 mockIPC。
 */

import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

// ---------------------------------------------------------------------------
// 模块加载引导：必须在引入任何 store 切片（及 `@/lib/tauri-commands.ts`）前执行，
// 确保模块求值期捕获的 `IS_TAURI_SHELL` 在测试全程为 `true`。
// 未注入时 `IS_TAURI_SHELL` 为 `false`，切片会走 Node 测试环境不存在对应 REST 端点的
// `webInvokeCmd`（fetch → /api/*）。
const g: any = globalThis;
if (!g.window) g.window = g;
if (!g.window.__TAURI_INTERNALS__) g.window.__TAURI_INTERNALS__ = {};

type EventListener = (event: any) => unknown;

export interface ShimGlobals {
  /** 强制修改 `document.visibilityState` 并派发 `visibilitychange` 事件。 */
  setVisibility(v: 'visible' | 'hidden'): void;
  /** 绑定的 `document` 桩对象（也可通过 `globalThis.document` 访问）。 */
  document: any;
  /** 清理桩注入的属性，恢复全局变量。 */
  teardown(): void;
}

export function installBrowserShim(): ShimGlobals {
  const g: any = globalThis;
  const hadWindow = 'window' in g;
  const prevWindow = g.window;
  const hadDocument = 'document' in g;
  const prevDocument = g.document;
  const hadLocalStorage = 'localStorage' in g;
  const prevLocalStorage = g.localStorage;
  const hadSetInterval = Object.prototype.hasOwnProperty.call(g, 'setInterval');
  const prevSetInterval = g.setInterval;
  const hadClearInterval = Object.prototype.hasOwnProperty.call(g, 'clearInterval');
  const prevClearInterval = g.clearInterval;

  // 切片调用 `window.setInterval` / `window.clearInterval`。
  // 将 `window` 别名到全局对象，保证属性可访问。
  if (!g.window) g.window = g;

  // localStorage：基于 Map 的实现，避免 `ui-slice` 的 `loadPrefs` 在模块求值期抛错。
  //
  // Node 自带一个 `localStorage` 全局变量，但它是个**空壳**（没有 getItem /
  // setItem），所以判据是「有没有那些方法」而不是「存不存在」—— 否则这个 shim
  // 会被 Node 的空壳挡住，而它装的那份 Map 从来没被用上。
  if (typeof g.localStorage?.getItem !== 'function') {
    const backing = new Map<string, string>();
    g.localStorage = {
      getItem(k: string) {
        return backing.has(k) ? (backing.get(k) as string) : null;
      },
      setItem(k: string, v: string) {
        backing.set(k, String(v));
      },
      removeItem(k: string) {
        backing.delete(k);
      },
      clear() {
        backing.clear();
      },
      key(i: number) {
        return Array.from(backing.keys())[i] ?? null;
      },
      get length() {
        return backing.size;
      },
    };
  }

  // 定时器函数：切片在可见性切换间共用单个定时器。测试内的调用计数由 `SetIntervalCounter`
  // 负责，此处确保 `window` 别名上的全局函数可解析。
  g.setInterval = ((handler: any, ms?: number, ...rest: any[]) => {
    return (prevSetInterval as any)(handler, ms, ...rest);
  }) as any;
  g.clearInterval = ((id: any) => {
    return (prevClearInterval as any)(id);
  }) as any;

  // 极简 `document`：切片只注册 `visibilitychange` 监听并读取 `visibilityState`，
  // 无需其他 DOM API。
  const listeners: Record<string, Set<EventListener>> = {};
  let visibility: 'visible' | 'hidden' = 'visible';
  const documentShim: any = {};
  Object.defineProperty(documentShim, 'visibilityState', {
    get: () => visibility,
    enumerable: true,
    configurable: true,
  });
  documentShim.addEventListener = (type: string, listener: EventListener) => {
    if (!listeners[type]) listeners[type] = new Set();
    listeners[type]!.add(listener);
  };
  documentShim.removeEventListener = (type: string, listener: EventListener) => {
    listeners[type]?.delete(listener);
  };
  documentShim.dispatchEvent = (event: { type: string } & Record<string, unknown>) => {
    const set = listeners[event.type];
    if (set) for (const h of set) h(event);
    return true;
  };
  g.document = documentShim;

  const setVisibility = (v: 'visible' | 'hidden') => {
    const prev = visibility;
    visibility = v;
    if (prev !== v) {
      documentShim.dispatchEvent({ type: 'visibilitychange' });
    }
  };

  const teardown = () => {
    if (hadWindow) g.window = prevWindow;
    else delete g.window;
    if (hadDocument) g.document = prevDocument;
    else delete g.document;
    if (hadLocalStorage) g.localStorage = prevLocalStorage;
    else delete g.localStorage;
    if (hadSetInterval) g.setInterval = prevSetInterval;
    else delete g.setInterval;
    if (hadClearInterval) g.clearInterval = prevClearInterval;
    else delete g.clearInterval;
  };

  return { setVisibility, document: documentShim, teardown };
}

export type IpcHandler = (cmd: string, args: any) => unknown | Promise<unknown>;

export function installIpcMock(handler: IpcHandler): void {
  // mockIPC 挂载 `window.__TAURI_INTERNALS__.invoke` 并透传回调结果；
  // 回调抛错会使 invoke promise reject，用于覆盖切片中的 catch 分支。
  mockIPC(async (cmd: string, args: any) => handler(cmd, args));
}

export function clearIpcMock(): void {
  try {
    clearMocks();
  } catch {
    // 未安装 mockIPC，清除为 no-op。
  }
}

/**
 * 统计两次快照间 `setInterval` 调用次数的计数器。
 * 配合 `installBrowserShim` 使用。
 */
export class SetIntervalCounter {
  private prev: any;
  private calls = 0;

  install() {
    const g: any = globalThis;
    this.prev = g.setInterval;
    // 箭头函数绑定当前实例，无需保留 `this` 别名。
    g.setInterval = ((handler: any, ms?: number, ...rest: any[]) => {
      this.calls++;
      return this.prev(handler, ms, ...rest);
    }) as any;
  }

  get count() {
    return this.calls;
  }

  reset() {
    this.calls = 0;
  }

  restore() {
    const g: any = globalThis;
    if (this.prev) g.setInterval = this.prev;
  }
}
