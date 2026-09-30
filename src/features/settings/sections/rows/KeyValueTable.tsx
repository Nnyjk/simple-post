import type { ReactNode } from 'react';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { IconTile } from '@/components/ui/icon-tile';
import type { HueConcept } from '@/lib/hue';
import { cn } from '@/lib/utils';
/**
 * KeyValueTable — the chrome shared by the env panel's two row tables
 * (BaseUrl definitions and environment variables).
 *
 * These two were 313 lines of near-verbatim copy: the same empty state,
 * the same `group flex items-start gap-3 px-4` row shell with its
 * first-row-suppresses-the-border rule, and the same full-width "add"
 * footer. Only the contents differed — icon, accent, labels, the
 * variable key's validation rule, and BaseUrl's extra "set default"
 * toggle. That shared shell now lives here; the per-table cells are
 * supplied through `renderRow`.
 *
 * The empty state and footer button are intentionally the same treatment
 * in both tables (a centred prompt + a small primary button when empty,
 * a hairline-divided text button at the foot when not) because they
 * express the same intent in both places: "this list is a set of named
 * slots you can add to".
 */

export interface KeyValueTableProps<T> {
  items: readonly T[];
  /** Stable React key. Required because `env.variables` allows a blank key. */
  keyOf: (item: T, index: number) => string;
  /** Shown centred when `items` is empty. */
  emptyText: string;
  /** Label on both the empty-state button and the footer button. */
  addText: string;
  onAdd: () => void;
  /** Row padding. BaseUrl rows are slightly taller than variable rows. */
  rowClassName?: string;
  /**
   * The row's cell contents, minus the row shell. Receives `flush` so a
   * row can suppress its own top border when it is the first one.
   */
  renderRow: (item: T, index: number, flush: boolean) => ReactNode;
}

export function KeyValueTable<T>({
  items,
  keyOf,
  emptyText,
  addText,
  onAdd,
  rowClassName,
  renderRow,
}: KeyValueTableProps<T>) {
  if (items.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 px-4 py-6 text-center">
        <div className="text-[11px] text-muted-foreground">{emptyText}</div>
        <Button variant="default" size="sm" className="h-7 gap-1 text-xs" onClick={onAdd}>
          <Plus className="h-3.5 w-3.5" />
          {addText}
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {items.map((item, idx) => {
        const flush = idx === 0;
        return (
          <div
            key={keyOf(item, idx)}
            className={cn(
              'group flex items-start gap-3 px-4',
              rowClassName,
              !flush && 'border-t border-border/60',
            )}
          >
            {renderRow(item, idx, flush)}
          </div>
        );
      })}
      <button
        type="button"
        onClick={onAdd}
        className="flex items-center gap-2 border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
      >
        <Plus className="h-3.5 w-3.5" />
        {addText}
      </button>
    </div>
  );
}

/**
 * The 36px bordered tile both tables put at the head of each row. The
 * `py-*` nudge differs per table, so it stays a parameter.
 */
export function RowTile({ concept, icon }: { concept: HueConcept; icon: ReactNode }) {
  return <IconTile size="lg" bordered concept={concept} icon={icon} />;
}
