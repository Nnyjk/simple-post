/**
 * Browser + Tauri IPC shim for running store slice tests under Node.
 *
 * Why this file exists
 * --------------------
 * `src/stores/*.ts` is designed to run inside the Tauri webview, so it
 * freely references `window.setInterval`, `document.visibilityState`,
 * `window.localStorage`, and `window.__TAURI_INTERNALS__.invoke` — none
 * of which exist in plain Node. This module exports a tiny shim that
 * gives each slice enough of a browser-shaped surface to run, plus a
 * Tauri-IPC mock delegated to `@tauri-apps/api/mocks#mockIPC`.
 *
 * Public surface:
 *   - `installBrowserShim()` — installs `window`, `document`,
 *      `localStorage` on `globalThis` if they aren't already there.
 *      Returns helpers to dispatch `visibilitychange` events from tests.
 *   - `installIpcMock(handler)` — installs
 *      `window.__TAURI_INTERNALS__.invoke` resolved through the supplied
 *      handler. Pass-through promises; throwing in the handler makes
 *      the invoke promise reject (which is how the slice's catch
 *      branches get exercised).
 *   - `clearIpcMock()` — clears the previous mockIPC state.
 *
 * The slice tests call these from `beforeEach`/`afterEach` so the
 * test-specific handler can be different per test (and not leak
 * across files).
 */

import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

// ---------------------------------------------------------------------------
// Module-load bootstrap — installed BEFORE any store slice (and therefore
// before `@/lib/tauri-commands.ts`) is imported, so the renderer-side
// `IS_TAURI_SHELL` constant — captured at module evaluation — resolves to
// `true` for the entire test run.
//
// Without this bootstrap `IS_TAURI_SHELL` would be `false` and the slices
// would route through `webInvokeCmd` (fetch → /api/*), which has no
// matching REST endpoint in the Node test environment.
// ---------------------------------------------------------------------------
const g: any = globalThis;
if (!g.window) g.window = g;
if (!g.window.__TAURI_INTERNALS__) g.window.__TAURI_INTERNALS__ = {};

type EventListener = (event: any) => unknown;

export interface ShimGlobals {
  /** Force the document.visibilityState; dispatches `visibilitychange`. */
  setVisibility(v: 'visible' | 'hidden'): void;
  /** The shim-bound `document` object (use `globalThis.document` too). */
  document: any;
  /** Reset everything the shim attached, restoring globals. */
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

  // The slice uses `window.setInterval` / `window.clearInterval`. Alias
  // `window` to the global object so the timer functions exist on the
  // property the code reaches through.
  if (!g.window) g.window = g;

  // localStorage — backing Map so ui-slice's `loadPrefs` doesn't throw
  // when it tries to read prefs during module evaluation.
  if (!g.localStorage) {
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

  // Timer functions — the slice stacks a single shared timer across
  // visibility transitions. Per-test interval counting lives in the
  // exported `SetIntervalCounter` below; here we only make sure the
  // window-aliased globals resolve.
  g.setInterval = ((handler: any, ms?: number, ...rest: any[]) => {
    return (prevSetInterval as any)(handler, ms, ...rest);
  }) as any;
  g.clearInterval = ((id: any) => {
    return (prevClearInterval as any)(id);
  }) as any;

  // Minimal `document` object. The slice adds a visibilitychange
  // listener and the handler reads `visibilityState`. We don't need
  // anything else from the DOM API.
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
  // mockIPC installs `window.__TAURI_INTERNALS__.invoke` returning
  // whatever the supplied callback resolves to. The callback can be
  // async; throwing makes the invoke promise reject, which is how
  // the slice's catch branches are exercised.
  mockIPC(async (cmd: string, args: any) => handler(cmd, args));
}

export function clearIpcMock(): void {
  try {
    clearMocks();
  } catch {
    // mockIPC was never installed; clearing is a no-op.
  }
}

/**
 * Counter that lets a test assert how many times `setInterval` was
 * called between two snapshots. Use alongside `installBrowserShim`,
 * which keeps its own counter for assertions during the same test.
 */
export class SetIntervalCounter {
  private prev: any;
  private calls = 0;

  install() {
    const g: any = globalThis;
    this.prev = g.setInterval;
    // Arrow callback: `this` stays bound to the instance, so no
    // `const that = this` alias is needed.
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
