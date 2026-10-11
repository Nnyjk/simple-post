import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import { MethodPill } from '@/components/ui/method-pill';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DropZone } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

export interface EndpointRowProps {
  endpoint: ResourceNode;
  /** The request configuration — the method pill reads from here. */
  detail: EndpointDetail;
  active: boolean;
  selected: boolean;
  multiSelect: boolean;
  isEditing: boolean;
  onClick: (e: ReactMouseEvent) => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onDragStart: (e: ReactDragEvent) => void;
  onDragEnd: (e: ReactDragEvent) => void;
  onDragOver: (e: ReactDragEvent) => void;
  onDragLeave: (e: ReactDragEvent) => void;
  onDrop: (e: ReactDragEvent) => void;
  dropHint: DropZone | null;
}

export function EndpointRow({
  endpoint,
  detail,
  active,
  selected,
  multiSelect,
  isEditing,
  onClick,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
  dropHint,
}: EndpointRowProps) {
  return (
    <div
      data-tree-row
      onContextMenu={onContextMenu}
      className={cn(
        // `active` means "this is the request open in the tab strip";
        // `selected` means "ticked for a bulk operation". They are
        // different things, so they read differently: active gets the
        // accent bar, selected gets the fill and the "已选" badge only.
        //
        // Both used to be a `bg-primary/…` tint, which made "active" and
        // "selected" the same colour and left the row reading as a link
        // rather than as a position in a list. A neutral fill plus a 3px
        // leading bar is the idiom both reference apps converge on, and it
        // keeps the saturated accent for the bar alone.
        'group mb-0.5 rounded-md',
        active && 'left-accent bg-surface-select',
        selected && !active && 'bg-surface-select',
      )}
    >
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
          onClick={onClick}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className={cn(
            // Same rule as ModuleRow: the name carries the row, the
            // method pill carries the data. Neither is muted.
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs text-foreground transition-colors',
            !active && !selected && 'hover:bg-overlay-hover',
          )}
        >
          {/* The HTTP method is the one icon this tree keeps: it is the only
              marker here that carries data the row does not already show.
              The generic endpoint tile that used to sit in front of it
              repeated what the row type already is, and pushed the method
              — the thing you scan for — one slot further right. */}
          <MethodPill
            method={detail.method}
            size="xs"
            variant="solid"
            title={`HTTP ${detail.method}`}
          />
          <InlineEdit
            value={endpoint.name}
            onSave={onSaveEdit}
            // Controlled by the tree's `editing` target, not by a local
            // double-click: creating an endpoint from the "+" menu has to
            // be able to drop the new row straight into rename, and that
            // signal arrives as tree state. `false` (the user finished or
            // cancelled) is the only transition this row acts on — opening
            // is the tree's business, and the row's own double-click
            // reaches it through `onStartEdit` above.
            editing={isEditing}
            onEditingChange={(open) => {
              if (!open) onCancelEdit();
            }}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{endpoint.name}</span>}
          />
          {multiSelect && selected && (
            <span className="shrink-0 rounded bg-primary/20 px-1.5 text-[11px] font-medium text-primary">
              已选
            </span>
          )}
        </button>
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}
