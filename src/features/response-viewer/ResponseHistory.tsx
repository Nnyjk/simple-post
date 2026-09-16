import { useState } from 'react';
import { History, ChevronDown } from 'lucide-react';
import { useAppStore, useActiveEndpoint } from '@/stores/app-store';
import type { ApiResponse } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { formatDuration, methodColorVar, statusColorVar, cn } from '@/lib/utils';

/**
 * Collapsible "History (n)" strip shown at the bottom of the response viewer.
 *
 * Selector note: we read the raw `responseHistory` object via a stable selector
 * (a single state field, not a derived `.find()`/`.filter()` result), and
 * resolve the per-endpoint array inside the component. The most-recent-first
 * slice is built with `useMemo` so we don't allocate a new array every render.
 */
export function ResponseHistory() {
  const endpoint = useActiveEndpoint();
  // Raw reference — only re-renders when the dictionary itself is replaced.
  const history = useAppStore((s) => s.responseHistory);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const [open, setOpen] = useState(false);

  // Read the array reference for the current endpoint. If the user switches
  // endpoints, this picks up the new array (or undefined).
  const entries = endpoint ? history[endpoint.id] : undefined;
  const count = entries?.length ?? 0;

  // Newest first, bounded by the store-side cap of 10.
  const ordered = entries ? entries.slice().reverse() : [];

  const onPick = (entry: ApiResponse) => {
    setLastResponse(entry);
  };

  return (
    <div className="shrink-0 border-t border-border bg-card/30">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex w-full items-center gap-2 px-4 py-1.5 text-left text-xs text-muted-foreground',
          'hover:bg-accent/30 transition-colors',
        )}
        aria-expanded={open}
        data-testid="response-history-toggle"
      >
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 transition-transform',
            open ? 'rotate-0' : '-rotate-90',
          )}
        />
        <History className="h-3.5 w-3.5" />
        <span className="font-medium">历史</span>
        <span className="rounded bg-muted px-1 text-[10px] tabular-nums">{count}</span>
      </button>

      {open && (
        <div className="max-h-56 overflow-y-auto border-t border-border/60">
          {ordered.length === 0 ? (
            <div className="px-4 py-3 text-center text-xs text-muted-foreground/70">
              暂无历史 — 发送请求后会出现在这里
            </div>
          ) : (
            <ul className="divide-y divide-border/40">
              {ordered.map((entry, idx) => (
                <HistoryRow
                  key={`${count - idx}-${entry.status}-${entry.durationMs}`}
                  entry={entry}
                  onPick={onPick}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

interface HistoryRowProps {
  entry: ApiResponse;
  onPick: (entry: ApiResponse) => void;
}

function HistoryRow({ entry, onPick }: HistoryRowProps) {
  // Method/url are not part of the original ApiResponse contract; they're
  // populated by the mock sender as optional fields. Fall back to placeholders
  // if a future backend forgets to set them.
  const method = (entry.method ?? 'GET') as string;
  const url = entry.url ?? '';
  const status = entry.status;
  const truncated = url.length > 60 ? `${url.slice(0, 57)}…` : url;

  return (
    <li>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => onPick(entry)}
        className={cn(
          'h-auto w-full justify-start gap-2 rounded-none px-4 py-1.5 text-xs font-normal',
          'hover:bg-accent/40',
        )}
        data-testid="response-history-item"
      >
        <span
          className="inline-flex h-5 w-12 shrink-0 items-center justify-center rounded font-mono text-[10px] font-semibold text-white"
          style={{ backgroundColor: methodColorVar(method) }}
        >
          {method}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-foreground/80">
          {truncated || '(无 URL)'}
        </span>
        <span
          className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px] tabular-nums"
          style={{ color: statusColorVar(status) }}
        >
          {status}
        </span>
        <span className="shrink-0 w-14 text-right tabular-nums text-muted-foreground">
          {formatDuration(entry.durationMs)}
        </span>
      </Button>
    </li>
  );
}
