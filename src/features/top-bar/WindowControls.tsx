/**
 * Self-drawn window controls — only effective when running inside the
 * Tauri shell (decorations: false). In the plain Vite/browser dev
 * mode these are rendered too but do nothing — clicking them is a
 * no-op via the catch in `safeCall`.
 *
 * Win11-style cluster: minimize, maximize/restore, close. Close is
 * the only one that uses the accent color (matches Explorer + VS Code
 * + every other Win11 app the user already trusts).
 *
 * Tauri 2 drag-region caveat: every interactive control inside the
 * TopBar's `data-tauri-drag-region` row must opt out with
 * `data-tauri-drag-region="false"`, otherwise the webview drag gesture
 * swallows the click.
 */
import { useEffect, useState } from 'react';
import { Minus, Square, Copy, X } from 'lucide-react';
import { Tooltip } from '@/components/ui/tooltip';

/**
 * Wrap a Tauri window call so the same component code can run in a
 * plain browser without throwing. Inside the Tauri shell the
 * `getCurrentWindow` import resolves to a real backend; outside, the
 * dynamic import + window.__TAURI_INTERNALS__ check fails fast and we
 * silently no-op.
 */
async function safeCall(fn: () => Promise<void>) {
  if (typeof window === 'undefined') return;
  // `__TAURI_INTERNALS__` is set on every Tauri 2 webview window.
  // Cheap guard so dev-in-browser doesn't hit an "ipc not available"
  // error every time you click a button.
  const tauriInternals = (window as { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
  if (!tauriInternals) return;
  try {
    await fn();
  } catch (e) {
    // Surface unexpected errors in devtools but don't disrupt the UI —
    // a broken minimize shouldn't take down the whole TopBar.
    // eslint-disable-next-line no-console
    console.warn('[WindowControls] tauri call failed:', e);
  }
}

const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function WindowControls() {
  // Track the maximized state live so the middle button can swap
  // between the "maximize" icon (single square) and the "restore"
  // icon (two overlapping squares). We subscribe to the Tauri
  // window's `resized` event and re-query the boolean — querying on
  // every click was tried earlier but missed double-click maximization
  // and any OS-level snap that didn't go through us.
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (
      typeof window === 'undefined' ||
      !(window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
    ) {
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const win = getCurrentWindow();
        if (cancelled) return;
        setIsMaximized(await win.isMaximized());
        unlisten = await win.onResized(async () => {
          setIsMaximized(await win.isMaximized());
        });
      } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('[WindowControls] onResized subscribe failed:', e);
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) {
        try {
          unlisten();
        } catch {
          // Already unsubscribed — fine.
        }
      }
    };
  }, []);

  const minimize = () =>
    safeCall(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().minimize();
    });

  // Explicitly query the maximized state and dispatch to the right
  // method. `toggleMaximize()` exists in the API but in some Tauri 2
  // builds the call is a no-op or races with the click handler — the
  // safe pattern is to read state first, then call the matching verb.
  const toggleMaximize = () =>
    safeCall(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const win = getCurrentWindow();
      const maximized = await win.isMaximized();
      if (maximized) {
        await win.unmaximize();
      } else {
        await win.maximize();
      }
    });

  const close = () =>
    safeCall(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    });

  return (
    <div className="flex items-center gap-0.5 pl-1">
      <Tooltip content="最小化" side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={minimize}
          aria-label="最小化"
          className="flex h-7 w-8 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Minus className="h-3.5 w-3.5" />
        </button>
      </Tooltip>

      <Tooltip content={isMaximized ? '还原' : '最大化'} side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={toggleMaximize}
          aria-label={isMaximized ? '还原窗口' : '最大化窗口'}
          className="flex h-7 w-8 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {/*
            Win11 uses two icon shapes for this button:
              - Single square  → "maximize"  (window is in normal state)
              - Overlapping squares → "restore"  (window is maximized)
            Switch between them live so the affordance always matches
            the action a click will perform.
          */}
          {isMaximized ? (
            <Copy className="h-3 w-3" />
          ) : (
            <Square className="h-3 w-3" />
          )}
        </button>
      </Tooltip>

      <Tooltip content="关闭" side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={close}
          aria-label="关闭窗口"
          className="flex h-7 w-8 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </Tooltip>
    </div>
  );
}