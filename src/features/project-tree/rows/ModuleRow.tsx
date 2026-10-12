import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from 'react';
import { ChevronRight, Plus } from 'lucide-react';
import type { ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import { Tooltip } from '@/components/ui/tooltip';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DropZone } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

export interface ModuleRowProps {
  module: ResourceNode;
  isEditing: boolean;
  onSelect: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
  onDragStart: (e: ReactDragEvent) => void;
  onDragEnd: (e: ReactDragEvent) => void;
  onDragOver: (e: ReactDragEvent) => void;
  onDragLeave: (e: ReactDragEvent) => void;
  onDrop: (e: ReactDragEvent) => void;
  dropHint: DropZone | null;
}

export function ModuleRow({
  module,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onPlusClick,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
  dropHint,
}: ModuleRowProps) {
  return (
    // Merge note: the merge already applied the `wip/module-baseurls`
    // side's removal of `rowRef` — the declaration is gone from the file
    // and `rowRef` appears nowhere else under `src/`, so keeping
    // `ref={rowRef}` on this line would not compile. This takes that
    // side's structure and keeps the aesthetic side's wrapper radius.
    // The wrapper-level `hover:bg-accent/60` is dropped to match
    // `CollectionRow`, which puts its hover on the inner button as
    // `bg-overlay-hover` instead.
    <div onContextMenu={onContextMenu} className="group mb-0.5 rounded-md">
      <div
        className={cn(
          'relative flex items-center gap-0.5 rounded-sm transition-colors',
          dropHint === 'into' && 'bg-primary/15 ring-1 ring-primary/40',
        )}
      >
        <button
          type="button"
          draggable
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          onDragOver={onDragOver}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
          onClick={onSelect}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className={cn(
            // The name is the reason this row exists, so it renders in
            // `--foreground`. It used to be `text-muted-foreground` and
            // only brighten on hover, which made the most important thing
            // in the tree the least legible thing in it. Secondary
            // information — the chevron — takes the muted tiers instead;
            // that is the whole point of having three of them.
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs font-medium text-foreground transition-colors',
            'hover:bg-overlay-hover',
          )}
        >
          <ChevronRight
            className={cn(
              'h-3 w-3 shrink-0 text-faint transition-transform',
              module.expanded && 'rotate-90 text-muted-foreground',
            )}
          />
          {/* No type icon here. The row used to carry a tinted folder tile
              (open/closed), but the disclosure chevron above already says
              "this expands", and every node kind in this tree is a
              container — the tile repeated what the indentation and the
              name beside it already convey. The one icon that carries
              information the row does not is the HTTP method on an
              endpoint, and that one stays.

              No size badge on the right either. It counted the children
              the chevron already reveals, and expanding is one click —
              a number that duplicates the tree's own structure is noise,
              not information. */}
          <InlineEdit
            value={module.name}
            onSave={onSaveEdit}
            // Controlled by the tree's `editing` target so a module
            // created from a dialog lands straight in rename. See the
            // same note in `EndpointRow.tsx`.
            editing={isEditing}
            onEditingChange={(open) => {
              if (!open) onCancelEdit();
            }}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{module.name}</span>}
          />
        </button>
        <div className="invisible flex shrink-0 items-center pr-1 group-hover:visible">
          <Tooltip content="新建" side="top">
            <button
              type="button"
              onClick={onPlusClick}
              onDoubleClick={(e) => e.stopPropagation()}
              aria-label="新建"
              className="flex h-5 w-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              <Plus className="h-3 w-3" />
            </button>
          </Tooltip>
        </div>
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}
