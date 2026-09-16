import { useEffect, useRef, useState } from 'react';
import { Loader2, RefreshCw } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
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
 * Local mock panel that visualises the (not-yet-real) MCP server process.
 *
 * Clicking the restart button flips `status` to `restarting`, waits
 * `RESTART_DELAY_MS`, then settles back to `running` with PID+1 and
 * requestsHandled+1. The timeout is tracked in a ref so a fast unmount
 * (e.g. tab switch + drawer close) cannot leak a setState into a
 * stale component.
 */
export function McpStatusPanel() {
  const [state, setState] = useState<McpUiState>(INITIAL_STATE);
  const timerRef = useRef<number | null>(null);

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
    <div className="flex items-center justify-between gap-3 rounded-md border border-border bg-card/30 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <StatusBadge status={state.status} />
        <span className="font-mono text-[11px] text-muted-foreground">
          PID {state.pid}
        </span>
        <span className="text-muted-foreground/60">·</span>
        <span className="text-[11px] text-muted-foreground">
          已处理请求 <span className="font-mono text-foreground">{state.requestsHandled}</span>
        </span>
      </div>
      <Button
        variant="outline"
        size="sm"
        onClick={handleRestart}
        disabled={state.status === 'restarting'}
        className="gap-1.5"
      >
        <RefreshCw
          className={cn(
            'h-3.5 w-3.5',
            state.status === 'restarting' && 'animate-spin',
          )}
        />
        重启 MCP
      </Button>
    </div>
  );
}

function StatusBadge({ status }: { status: McpRuntimeStatus }) {
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
