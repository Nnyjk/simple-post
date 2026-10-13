import { useEffect, useMemo, useState } from 'react';
import { History, ChevronDown } from 'lucide-react';
import { useAppStore, useActiveEndpointResource } from '@/stores/app-store';
import { DRAFT_ID_PREFIX } from '@/stores/store-types';
import type { ResponseHistoryEntry } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { formatDuration, methodColorVar, statusColorVar, cn } from '@/lib/utils';

/**
 * Collapsible "History (n)" strip shown at the bottom of the response viewer.
 *
 * The rows come from the store's `responseHistory`, which is backed by the
 * `response_history` table and loaded per endpoint — so they survive a
 * relaunch, and every one of them carries a server-stamped `id` and
 * `createdAt` (see `ResponseHistoryEntry`).
 *
 * Selector note: we read the raw `responseHistory` object via a stable selector
 * (a single state field, not a derived `.find()`/`.filter()` result), and
 * resolve the per-endpoint array inside the component. The most-recent-first
 * slice is built with `useMemo` so we don't allocate a new array every render.
 */
export function ResponseHistory() {
  // History is keyed by endpoint id, and a draft endpoint is keyed by its
  // `draft:` id — so an unsaved endpoint still accumulates its own send
  // history instead of showing nothing.
  const endpoint = useActiveEndpointResource()?.node ?? null;
  // Raw reference — only re-renders when the dictionary itself is replaced.
  const history = useAppStore((s) => s.responseHistory);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const loadResponseHistory = useAppStore((s) => s.loadResponseHistory);
  const clearResponseHistory = useAppStore((s) => s.clearResponseHistory);
  const [open, setOpen] = useState(false);

  const endpointId = endpoint?.id ?? null;

  // Read this endpoint's stored history when it becomes the one on screen.
  //
  // The effect is the trigger rather than an action-level hook, and that
  // is not a style choice. `activeTabId` has **three** writers — `openTab`
  // (which is how the tree and the command palette open an endpoint),
  // `setActiveTab` (the tab strip) and `closeTab`'s fallback — so a hook
  // placed on any one of them silently misses the others, and a history
  // that loads only when the user switches tabs *back* to an endpoint is
  // indistinguishable from one that never loads. A component effect keys
  // off the value the UI actually resolved, so every path is covered by
  // construction.
  //
  // `id in history` is the "already have it" guard: switching back to a tab
  // should not refetch rows the store is holding, and `clearResponseHistory`
  // deletes the key, so a cleared endpoint does get re-read.
  useEffect(() => {
    if (!endpointId || endpointId.startsWith(DRAFT_ID_PREFIX)) return;
    if (endpointId in useAppStore.getState().responseHistory) return;
    void loadResponseHistory(endpointId);
  }, [endpointId, loadResponseHistory]);

  // Read the array reference for the current endpoint. If the user switches
  // endpoints, this picks up the new array (or undefined).
  const entries = endpointId ? history[endpointId] : undefined;
  const count = entries?.length ?? 0;

  // Newest first, for display only.
  //
  // The store holds newest-*last* — `appendCapped` pushes onto the end and
  // drops the front, and `mergeLoaded` normalises a server read into the
  // same order — so this reverse is the single place the order flips, and
  // the only reason the store is allowed to disagree with the reply it
  // got. Reversing anywhere else, or letting the reply's own
  // `created_at DESC` order through unchanged, silently inverts the
  // strip: rows paint oldest-first, and the next send drops the newest row
  // from the cache while SQLite trims the oldest.
  const ordered = useMemo(() => (entries ? entries.slice().reverse() : []), [entries]);

  // One reading of the clock per render, so the ten rows below cannot
  // disagree about what "3 分钟前" means. Not a `useMemo`: it depends on
  // nothing in state, and memoising it would freeze the stamps until
  // something *else* re-rendered.
  const now = Date.now();

  const onPick = (entry: ResponseHistoryEntry) => {
    // Unwrap: the viewer shows a response, not a stored row. A clipped
    // row carries `bodyTruncated`, but the body it hands over is already
    // the clipped one the server stored — nothing to strip here.
    setLastResponse(entry.response);
  };

  const onClear = () => {
    if (!endpointId) return;
    void clearResponseHistory(endpointId);
  };

  return (
    <div className="shrink-0 border-t border-border bg-card/30">
      <div className="flex items-center">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className={cn(
            'flex flex-1 items-center gap-2 px-4 py-1.5 text-left text-xs text-muted-foreground',
            'hover:bg-accent/30 transition-colors',
          )}
          aria-expanded={open}
          data-testid="response-history-toggle"
        >
          <ChevronDown
            className={cn('h-3.5 w-3.5 transition-transform', open ? 'rotate-0' : '-rotate-90')}
          />
          <History className="h-3.5 w-3.5" />
          <span className="font-medium">历史</span>
          <span className="rounded bg-muted px-1 text-[10px] tabular-nums">{count}</span>
        </button>
        {/* Sibling of the toggle, not a child: nesting a button inside the
            toggle would both be invalid HTML and expand the panel on
            every clear. `count > 0` because an empty strip has nothing
            to clear and the affordance would only be noise. */}
        {count > 0 && (
          <Button
            variant="ghost"
            size="xs"
            onClick={onClear}
            className="mr-2 shrink-0 text-muted-foreground"
            data-testid="response-history-clear"
          >
            清空
          </Button>
        )}
      </div>

      {open && (
        <div className="max-h-56 overflow-y-auto border-t border-border/60">
          {ordered.length === 0 ? (
            <div className="px-4 py-3 text-center text-xs text-muted-foreground/70">
              暂无历史 — 发送请求后会出现在这里
            </div>
          ) : (
            <ul className="divide-y divide-border/40">
              {ordered.map((entry) => (
                <HistoryRow key={entry.id} entry={entry} now={now} onPick={onPick} />
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * "刚刚 / 12 秒前 / 3 分钟前", falling back to a clock time once "x 分钟前"
 * stops carrying information.
 *
 * Local to this file rather than `src/lib/utils.ts`: it is the only caller,
 * and `utils.ts` is otherwise formatters for a payload (bytes, ms, status
 * hue) with no notion of "now". Anything else that starts rendering
 * relative times is the moment to promote it.
 */
function formatRelativeTime(createdAt: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - createdAt) / 1000));
  if (seconds < 10) return '刚刚';
  if (seconds < 60) return `${seconds} 秒前`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return new Date(createdAt).toLocaleString();
}

interface HistoryRowProps {
  entry: ResponseHistoryEntry;
  /** One clock reading shared by every row in the list. */
  now: number;
  onPick: (entry: ResponseHistoryEntry) => void;
}

function HistoryRow({ entry, now, onPick }: HistoryRowProps) {
  // Method/url are not part of the original ApiResponse contract; they're
  // populated by the sender as optional fields. Fall back to placeholders
  // if a future backend forgets to set them.
  const method = (entry.response.method ?? 'GET') as string;
  const url = entry.response.url ?? '';
  const status = entry.response.status;
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
        {/* The server clipped this body at 256 KiB and dropped `bodyJson`
            with it, so the viewer will show half a document. Without a
            marker the user cannot tell that from the endpoint having
            returned half a document. `--warning-foreground` is the text
            step of the amber ramp (lightened in the dark scope) — see
            the semantic-token pairing note in AGENTS.md; pairing a
            `-foreground` token with a light *surface* is the trap. */}
        {entry.bodyTruncated && (
          <span
            className="shrink-0 rounded bg-warning/15 px-1.5 py-0.5 text-[10px] text-warning-foreground"
            title="响应体超过 256 KiB，已截断存储"
            data-testid="response-history-truncated"
          >
            已截断
          </span>
        )}
        <span
          className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px] tabular-nums"
          style={{ color: statusColorVar(status) }}
        >
          {status}
        </span>
        <span className="shrink-0 w-14 text-right tabular-nums text-muted-foreground">
          {formatDuration(entry.response.durationMs)}
        </span>
        {/* Third text level: present, not actionable — the row carries the
            timestamp because a history that survives a relaunch is a
            record of *when*, not just of what came back. */}
        <span className="shrink-0 w-16 text-right tabular-nums text-faint">
          {formatRelativeTime(entry.createdAt, now)}
        </span>
      </Button>
    </li>
  );
}
