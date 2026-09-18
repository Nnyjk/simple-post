/**
 * MCP slice — owns the polling lifecycle for the shared MCP daemon
 * status snapshot.
 *
 * Responsibilities:
 *   - Holds `mcpStatus` (null until the first successful poll).
 *   - Owns the SINGLETON 3s `setInterval` so multiple components can
 *     each call `startMcpPolling()` from their `useEffect` without
 *     stacking timers (the previous audit replaced the duplicate
 *     setInterval that used to live in TopBar AND McpStatusPanel).
 *   - Installs a `document.visibilitychange` listener that pauses
 *     the timer while the tab is hidden — there's nothing for the
 *     user to look at, and the daemon state doesn't change between
 *     wake-ups.
 *   - Lets consumers force-refresh (`refreshMcpStatus`), restart
 *     (`restartMcp`), and dedupe the auto-open "restart" dialog
 *     (`restartDialogSeen`).
 *
 * The slice takes `(set, get, store)` from a `StateCreator<AppState, ...>`
 * so it shares the same backing Zustand store as the other slices.
 * It NEVER imports another slice module directly — its only coupling
 * to the rest of the store is the shared `AppState` type.
 */
import type { StateCreator } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { COMMANDS } from '@/lib/tauri-commands';
import type { McpRuntimeStatus, McpUiState, RestartResult } from '@/lib/mcp-types';
import type { AppState } from './app-store';

/**
 * Polling cadence for the shared MCP daemon snapshot. Single source of
 * truth — both `startMcpPolling` (in the store) and the legacy
 * `setInterval` it replaced used this value.
 */
export const MCP_REFRESH_INTERVAL_MS = 3000;

// Browser-only — holds the bound `visibilitychange` handler so the
// store can detach it on `stopMcpPolling`. The store action is the
// only consumer so a module-scoped variable (no React state) is fine.
let _mcpVisibilityHandler: ((this: Document, ev: Event) => unknown) | null = null;
function _setVisibilityHandler(
  h: ((this: Document, ev: Event) => unknown) | null,
): void {
  _mcpVisibilityHandler = h;
}
function _getVisibilityHandler():
  | ((this: Document, ev: Event) => unknown)
  | null {
  return _mcpVisibilityHandler;
}

/**
 * Canonical "not running" snapshot for the MCP daemon. Every fallback
 * branch in `restartMcp` that constructs a fresh `McpUiState` (because
 * the polled snapshot hasn't landed yet) spreads this constant and
 * overrides `status` (and `lastError` on the failure path). Keeping
 * the field list in one place is what stops the previous audit's
 * finding F-E3 from regressing — adding a new `McpUiState` field
 * should only touch this constant plus its type declaration in
 * `@/lib/mcp-types`, not three action bodies scattered below.
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

/** State portion owned by the MCP slice. */
export interface McpSliceState {
  /**
   * Shared MCP daemon snapshot, polled every 3s by exactly one
   * `setInterval` regardless of how many components consume it.
   * `null` until the first successful poll (renderers render a
   * loading-style state for this).
   */
  mcpStatus: McpUiState | null;
  /** Browser-only. The active 3s timer id, or `null` when idle. */
  _mcpTimer: number | null;
  /**
   * Last `status` for which the ConnectionSection's auto-open restart
   * dialog has already fired. Used to dedupe — when the daemon
   * reports `crashed` (or `unhealthy`) twice in a row, the dialog
   * should appear once, not twice. Lives on the store so the
   * auto-open effect survives remounts of the settings tab.
   */
  restartDialogSeen: McpRuntimeStatus | null;
}

/** Action portion owned by the MCP slice. */
export interface McpSliceActions {
  /**
   * Begin the 3-second MCP polling loop if it isn't already running.
   * Idempotent — calling this from N components still results in exactly
   * one `setInterval` and one IPC round-trip per tick.
   *
   * Also installs a `document.visibilitychange` listener that pauses
   * the timer while the tab is hidden (no point polling for users
   * who can't see the badge). The listener is removed by
   * `stopMcpPolling`.
   */
  startMcpPolling: () => void;
  /**
   * Stop the polling loop and detach the visibility listener. Pair
   * every `startMcpPolling` (typically from a `useMcpStatus` effect
   * mount) with a `stopMcpPolling` cleanup so we don't leak.
   */
  stopMcpPolling: () => void;
  /**
   * Force a single refresh from the daemon and push the result into
   * `mcpStatus`. On failure we only log (so the UI keeps rendering
   * the last-known good state) — the bootstrap path's silent
   * `return []` behaviour was the wrong default here.
   */
  refreshMcpStatus: () => Promise<void>;
  /**
   * Trigger the daemon restart command. The action is idempotent at
   * the *UI* level (the component is the caller, so the button is
   * disabled while a restart is already in flight). On failure we
   * flip `mcpStatus.lastError` so the crashed dialog can show the
   * underlying cause.
   */
  restartMcp: () => Promise<void>;
  /**
   * Record that the auto-open restart dialog has been shown for the
   * given status. Idempotent — the ConnectionSection effect's dedupe
   * logic calls this when the user dismisses the dialog so the next
   * occurrence of the same status doesn't immediately reopen it.
   */
  markRestartDialogSeen: (status: McpRuntimeStatus) => void;
  /**
   * Record the most recent daemon-reported error on
   * `mcpStatus.lastError`. Used by the `daemon://crashed` event
   * listener in McpStatusPanel to surface the daemon-supplied error
   * string immediately instead of waiting for the next 3-second poll
   * to land. Passing `null` clears the field (e.g. after a successful
   * restart). No-op when there is no `mcpStatus` snapshot yet — the
   * panel tolerates the loading gap. Centralising this keeps the
   * renderer from calling `setState` directly on `mcpStatus`, which
   * would bypass every invariant the slice doc-comment promises.
   */
  recordLastError: (lastError: string | null) => void;
  /**
   * Read the persisted `mcp.auto_start` preference. `null` until the
   * first read lands; the panel renders a disabled switch in that
   * window. See `docs/distribution-modes-2026-09-18.md` §5.
   */
  mcpAutoStart: boolean | null;
  /**
   * Persist `mcp.auto_start`. When the user enables the toggle we
   * also ask the daemon to start (idempotent) so they don't have to
   * take a second action. When they disable it we leave the running
   * daemon alone — auto_start is about whether the daemon starts on
   * launch, not a runtime kill switch.
   */
  setMcpAutoStart: (value: boolean) => Promise<void>;
  /**
   * Re-read the `mcp.auto_start` preference from the backend. The
   * ConnectionSection calls this on mount so the toggle reflects the
   * persisted value (defaults to `false` when the row is absent).
   */
  refreshMcpAutoStart: () => Promise<void>;
}

export type McpSlice = McpSliceState & McpSliceActions;

export const createMcpSlice: StateCreator<AppState, [], [], McpSlice> = (
  set,
  get,
) => ({
  // MCP daemon state — populated lazily by `startMcpPolling()`. The
  // initial `null` lets consumers render a "loading" affordance until
  // the first `refreshMcpStatus()` round-trip completes.
  mcpStatus: null,
  _mcpTimer: null,
  restartDialogSeen: null,
  /**
   * Persisted `mcp.auto_start` preference. Populated by the
   * `ConnectionSection` effect via `refreshMcpAutoStart`; null until
   * the first IPC. Defaults to `false` server-side, so users who
   * haven't touched the toggle yet will see `false` after one read.
   */
  mcpAutoStart: null,

  // ---------- MCP daemon polling ----------
  // Three collaborators participate in the singleton 3s tick:
  //   - `useMcpStatus` calls start()/stop() in its mount/cleanup
  //   - `McpStatusPanel` owns the `daemon://crashed` event listener,
  //     which force-refreshes via `refreshMcpStatus` on a crash.
  //   - `ConnectionSection` calls `refreshMcpStatus` after a manual
  //     restart so the badge flips back to `running` immediately.
  startMcpPolling: () => {
    const state = get();
    if (state._mcpTimer !== null) return; // idempotent — only one timer ever
    const tick = () => {
      void state.refreshMcpStatus();
    };
    const timer = window.setInterval(tick, MCP_REFRESH_INTERVAL_MS);
    set({ _mcpTimer: timer });
    // Kick off the first read immediately so the UI doesn't sit on
    // `mcpStatus: null` for up to 3s after the panel mounts.
    tick();

    // Pause while the tab is hidden — there's nothing to display and
    // the daemon doesn't change between wake-ups. The listener is
    // owned by the store (module-scoped handler ref) so every call to
    // start() doesn't add a new one and stop() can detach cleanly.
    if (typeof document !== 'undefined' && _getVisibilityHandler() === null) {
      const handler = () => {
        if (document.visibilityState === 'hidden') {
          // Pause: clear but keep the handler ref so start() can
          // resume from the visibilitychange→'visible' branch.
          const cur = get();
          if (cur._mcpTimer !== null) {
            window.clearInterval(cur._mcpTimer);
            set({ _mcpTimer: null });
          }
        } else if (document.visibilityState === 'visible') {
          // Resume — start() is idempotent so we can call it freely.
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
    // Detach visibility listener — only the last unmounted consumer
    // should tear it down, but it's idempotent so calling it here
    // is safe.
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
      // The Rust `McpStatusDto` carries `#[serde(rename_all =
      // "camelCase")]` plus the new `status` and
      // `current_connections` fields, so the wire shape now matches
      // `McpUiState` one-for-one. The single cast is the only
      // escape hatch — the renderer trusts the daemon's lifecycle
      // string and the badge falls back to `'stopped'` for any
      // unknown value, so a runtime mismatch is impossible to
      // surface as a TS error.
      const dto = (await invoke<unknown>(COMMANDS.mcpStatus)) as McpUiState;
      set({ mcpStatus: dto });
    } catch (e) {
      // Daemon unreachable (cold start, dev mode without the daemon
      // supervisor, or an IPC race) — log and keep whatever the last
      // good state was. We deliberately do NOT clobber `mcpStatus`
      // with a "stopped" snapshot, because that would mask a real
      // daemon crash with a transient IPC failure.
      console.warn('[store] mcp_status poll failed', e);
    }
  },
  restartMcp: async () => {
    // The renderer's role here is to flip the badge to "restarting"
    // immediately, fire the command, then refresh so the new PID /
    // status is visible. The UI button's `disabled` state is the
    // caller's job (it tracks in-flight via local state because the
    // round-trip can be 100ms–2s and we want to disable the moment
    // the user clicks).
    const prev = get().mcpStatus;
    set({
      mcpStatus: prev
        ? { ...prev, status: 'restarting' }
        : { ...EMPTY_MCP_STATE, status: 'restarting' },
    });
    try {
      const result: RestartResult = await invoke<RestartResult>(
        COMMANDS.mcpDaemonRestart,
      );
      if (!result.ok) {
        // Rust swallowed the underlying `AppError` and returned
        // `{ ok: false, pid: null, port: null }`. Surface the same
        // `crashed` badge the previous error path did so the dialog
        // can pick it up; we don't have an error string from Rust so
        // we leave `lastError` unset and let the next poll surface
        // whatever the daemon itself reports.
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
      // Surface the failure on the badge — the dialog reads lastError.
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
  markRestartDialogSeen: (status) => {
    set({ restartDialogSeen: status });
  },
  recordLastError: (lastError) => {
    set((s) => {
      if (!s.mcpStatus) return {};
      // `McpUiState.lastError` is optional (`string | undefined`),
      // not `string | null`. Coerce the explicit `null` "clear"
      // sentinel from the panel's listener into `undefined` so the
      // spread type-checks under strict mode.
      const next: string | undefined = lastError ?? undefined;
      if (s.mcpStatus.lastError === next) return {};
      return { mcpStatus: { ...s.mcpStatus, lastError: next } };
    });
  },
  /**
   * Read the persisted auto-start preference. Silent on failure — the
   * connection panel tolerates the row being unreadable by leaving
   * the switch in its indeterminate `null` state.
   */
  refreshMcpAutoStart: async () => {
    try {
      const v = await invoke<boolean>(COMMANDS.mcpAutoStartGet);
      set({ mcpAutoStart: v });
    } catch (e) {
      console.warn('[store] mcp_auto_start_get failed', e);
    }
  },
  setMcpAutoStart: async (value) => {
    // Optimistic flip — the panel disables the switch during the
    // round-trip and we want it to reflect the user's intent
    // immediately. The setter throws on failure so the panel can
    // roll the switch back and surface the error.
    set({ mcpAutoStart: value });
    try {
      await invoke<boolean>(COMMANDS.mcpAutoStartSet, { value });
      // If we just turned auto-start on, the daemon may have started
      // — refresh the snapshot so the badge updates without waiting
      // for the next 3s tick.
      if (value) {
        await get().refreshMcpStatus();
      }
    } catch (e) {
      // Roll back + re-throw so the panel can show a toast.
      set({ mcpAutoStart: !value });
      throw e;
    }
  },
});