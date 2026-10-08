import type { ReactNode } from 'react';
import { Plus } from 'lucide-react';
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
 * The add control is one element shared by both states. It used to be
 * two: a centred solid `variant="default"` button inside the empty state,
 * and a hairline-divided text button at the foot of a populated list. Both
 * say "this list is a set of named slots you can add to", and both tables
 * rendered them side by side on the same screen — so the empty one read as
 * a different, more prominent action than the identical action you get one
 * row later. Now the empty state keeps only its prompt and renders the very
 * same footer button, so the control cannot drift between the two states.
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
  // One control, both states. Hoisting it out of the branches is the point:
  // a per-branch copy is exactly how the empty state came to look different
  // from the populated one in the first place.
  const addControl = (
    <button
      type="button"
      onClick={onAdd}
      className="flex items-center gap-2 border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
    >
      <Plus className="h-3.5 w-3.5" />
      {addText}
    </button>
  );

  if (items.length === 0) {
    return (
      <div className="flex flex-col">
        <div className="px-4 py-6 text-center text-[11px] text-muted-foreground">{emptyText}</div>
        {addControl}
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
      {addControl}
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
