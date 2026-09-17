import { useEffect, useRef, useState } from 'react';
import { Loader2, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';

export type McpRuntimeStatus = 'running' | 'restarting' | 'stopped';

export interface McpUiState {
  status: McpRuntimeStatus;
  pid: number;
  requestsHandled: number;
}

const INITIAL_STATE: McpUiState = {
  status: 'running',
  pid: 12345,
  requestsHandled: 1247,
};

const RESTART_DELAY_MS = 800;

/**
 * Restart control + status broadcasting for the MCP hero card.
 *
 * Owns the mock runtime state (PID, request count, restart-in-flight)
 * and broadcasts it via `onStateChange` so the parent card can render
 * the status pill, the PID, and the request count without each of them
 * needing its own useState hook. The timeout is tracked in a ref so a
 * fast unmount (e.g. tab switch + drawer close) cannot leak a setState
 * into a stale component.
 */
export function McpStatusPanel({
  onStateChange,
}: {
  onStateChange?: (state: McpUiState) => void;
}) {
  const [state, setState] = useState<McpUiState>(INITIAL_STATE);
  const timerRef = useRef<number | null>(null);

  // Push every state change up so the parent can render the hero card.
  useEffect(() => {
    onStateChange?.(state);
  }, [state, onStateChange]);

  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, []);

  const handleRestart = () => {
    if (state.status === 'restarting') return;
    setState((prev) => ({ ...prev, status: 'restarting' }));
    timerRef.current = window.setTimeout(() => {
      setState((prev) => ({
        status: 'running',
        pid: prev.pid + 1,
        requestsHandled: prev.requestsHandled + 1,
      }));
      timerRef.current = null;
    }, RESTART_DELAY_MS);
  };

  return (
    <Button
      variant="outline"
      size="sm"
      onClick={handleRestart}
      disabled={state.status === 'restarting'}
      className="gap-1.5"
    >
      {state.status === 'restarting' ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
      ) : (
        <RefreshCw className="h-3.5 w-3.5" />
      )}
      {state.status === 'restarting' ? '重启中…' : '重启 MCP'}
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
  if (status === 'restarting') {
    return (
      <Badge variant="warning">
        <Loader2 className="h-3 w-3 animate-spin" />
        restarting…
      </Badge>
    );
  }
  return <Badge variant="muted">stopped</Badge>;
}

/** Re-export a tiny `<StateSummary>` helper that consumes the same state
 *  object the panel broadcasts. The connection section composes both. */
export function StateSummary({ state }: { state: McpUiState }) {
  return (
    <span className="inline-flex items-center gap-2">
      <StatusBadge status={state.status} />
      <span className="font-mono text-foreground/80">PID {state.pid}</span>
      <span className="text-muted-foreground/60">·</span>
      <span className="text-muted-foreground">
        已处理 <span className="font-mono text-foreground">{state.requestsHandled.toLocaleString()}</span> 次请求
      </span>
    </span>
  );
}