import { useEffect, useRef, useState } from 'react';
import { Loader2, RefreshCw } from 'lucide-react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { useAppStore } from '@/stores/app-store';
import { useMcpStatus } from '@/lib/use-mcp-status';
import type { McpRuntimeStatus, McpUiState } from '@/lib/mcp-types';

// Re-export so existing callers (ConnectionSection, Settings tab etc.)
// don't need to learn about the internal split between the panel and
// the shared mcp-types module.
export type { McpRuntimeStatus, McpUiState };

/**
 * Grace period after a restart invoke before we re-enable the button.
 * Keeps the UI from flashing "clickable" during a flaky round-trip.
 */
const RESTART_GRACE_MS = 2000;

/**
 * Restart control for the MCP daemon.
 *
 * Source-of-truth ownership:
 *   - The 3-second polling loop lives in the Zustand store
 *     (`useAppStore.startMcpPolling` / `refreshMcpStatus`). Any
 *     component mounting `useMcpStatus` participates in the same
 *     singleton loop, so there is no duplicate `setInterval` here.
 *   - This panel keeps ONE piece of daemon-local logic: the
 *     `daemon://crashed` event listener. When Rust emits the event
 *     we eagerly call `refreshMcpStatus` so the badge flips to
 *     `crashed` immediately (not on the next 3s tick).
 *   - The button itself calls `useAppStore.getState().restartMcp()`
 *     — the optimistic "restarting" flip lives in the store so a
 *     concurrent restart from the dialog button produces the same
 *     visible state.
 *
 * The local `restarting` state is purely cosmetic (keeps the button
 * disabled while the invoke is in flight and during the grace
 * period) — it does NOT shadow `mcpStatus.status === 'restarting'`
 * from the daemon, which is what the badge renders.
 */
export function McpStatusPanel() {
  const mcpStatus = useMcpStatus();
  const restartMcp = useAppStore((s) => s.restartMcp);

  // Cosmetic local state — debounces the button's click → disable
  // transition so a flaky invoke doesn't cause flicker. Mirrors the
  // grace window the original implementation used.
  const [isRestartingLocal, setIsRestartingLocal] = useState(false);
  const graceRef = useRef<number | null>(null);

  // Event-driven refresh + grace-timer cleanup, hosted in a single
  // effect. Subscribes to `daemon://crashed` once on mount and
  // unsubscribes on unmount; the listener is idempotent — Rust
  // publishes the event, we just call `refreshMcpStatus` to pick up
  // the new (crashed) state without waiting for the next 3s tick.
  useEffect(() => {
    let cancelled = false;
    const unlistenRefs: UnlistenFn[] = [];

    listen<{ lastError?: string }>('daemon://crashed', (e) => {
      // Eager refresh — don't wait for the next 3s tick — and surface
      // the daemon-supplied error string immediately so the
      // crashed/restart dialog can show it without waiting for the
      // next daemon poll. The `lastError` write goes through the
      // slice's `recordLastError` action (no direct `setState`), which
      // keeps the renderer from bypassing the slice invariants the
      // mcp-slice doc-comment promises (no-op when no snapshot yet,
      // short-circuit when the value hasn't changed).
      const lastError = e.payload?.lastError ?? null;
      void useAppStore
        .getState()
        .refreshMcpStatus()
        .then(() => {
          useAppStore.getState().recordLastError(lastError);
        });
    })
      .then((u) => {
        if (cancelled) {
          u();
          return;
        }
        unlistenRefs.push(u);
      })
      .catch((e) => {
        console.error('[MCP] listen(daemon://crashed) failed', e);
      });

    return () => {
      cancelled = true;
      for (const u of unlistenRefs) u();
      // Clear any pending grace timer so an unmount mid-restart
      // doesn't toggle the button state on a dead instance.
      if (graceRef.current !== null) {
        window.clearTimeout(graceRef.current);
        graceRef.current = null;
      }
    };
  }, []);

  async function handleClick() {
    if (isRestartingLocal) return;
    setIsRestartingLocal(true);
    try {
      await restartMcp();
    } catch {
      // restartMcp already logged + flipped the badge to `crashed`.
      // We just want the button to become clickable again so the
      // user can retry from the same place.
    } finally {
      if (graceRef.current !== null) window.clearTimeout(graceRef.current);
      graceRef.current = window.setTimeout(() => {
        setIsRestartingLocal(false);
        graceRef.current = null;
      }, RESTART_GRACE_MS);
    }
  }

  const status: McpRuntimeStatus = mcpStatus?.status ?? 'stopped';
  const isStopped = status === 'stopped' || status === 'crashed' || status === 'unhealthy';
  // Disable the button during the local grace period OR while the
  // daemon itself says it is restarting.
  const disabled = isRestartingLocal || status === 'restarting' || status === 'starting';

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={() => {
        void handleClick();
      }}
      disabled={disabled}
      className="gap-1.5"
    >
      {disabled ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
      ) : (
        <RefreshCw className={cn('h-3.5 w-3.5', isStopped && 'text-destructive')} />
      )}
      {disabled ? '重启中…' : '重启 MCP'}
    </Button>
  );
}

export function StatusBadge({ status }: { status: McpRuntimeStatus }) {
  if (status === 'running') {
    return (
      <Badge variant="success">
        <span className="mr-1 h-1.5 w-1.5 rounded-full bg-emerald-400" />
        running
      </Badge>
    );
  }
  if (status === 'restarting' || status === 'starting') {
    return (
      <Badge variant="warning">
        <Loader2 className="h-3 w-3 animate-spin" />
        restarting…
      </Badge>
    );
  }
  if (status === 'crashed' || status === 'unhealthy') {
    return (
      <Badge variant="destructive">
        <span className="mr-1 h-1.5 w-1.5 rounded-full bg-destructive" />
        {status}
      </Badge>
    );
  }
  return <Badge variant="muted">stopped</Badge>;
}

/** Tiny `<StateSummary>` helper that consumes an `McpUiState` snapshot
 *  directly. ConnectionSection passes the store-fed state. Tolerates
 *  `null` for the brief window before the first daemon poll completes. */
export function StateSummary({ state }: { state: McpUiState | null }) {
  if (!state) {
    return (
      <span className="inline-flex items-center gap-2 text-muted-foreground">
        <span className="h-1.5 w-1.5 rounded-full bg-zinc-500" />
        <span className="text-[11px]">连接中…</span>
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-2">
      <StatusBadge status={state.status} />
      {state.pid !== null && (
        <>
          <span className="font-mono text-foreground/80">PID {state.pid}</span>
          <span className="text-muted-foreground/60">·</span>
        </>
      )}
      <span className="text-muted-foreground">
        已处理{' '}
        <span className="font-mono text-foreground">{state.requestsHandled.toLocaleString()}</span>{' '}
        次请求
      </span>
    </span>
  );
}
