import {
  useRef,
  type DragEvent as ReactDragEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react';
import { ChevronRight, FolderClosed, FolderOpen, Plus } from 'lucide-react';
import type { Module } from '@/types/domain';
import { cn } from '@/lib/utils';
import { Tooltip } from '@/components/ui/tooltip';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DropZone } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

export interface ModuleRowProps {
  module: Module;
  collectionCount: number;
  endpointCount: number;
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
  collectionCount,
  endpointCount,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onContextMenu,
  onPlusClick,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
  dropHint,
}: ModuleRowProps) {
  const rowRef = useRef<HTMLDivElement>(null);
  // Bind the per-row element into the handlers. The closures captured
  // by `makeDragOverHandler` / `makeDropHandler` (above) were created
  // with `null` for the element, since the element only exists at
  // render time. We re-wrap them here so the runtime can read the
  // actual bounding rect.
  const handleDragOver = (e: ReactDragEvent) => onDragOver(e);
  const handleDrop = (e: ReactDragEvent) => onDrop(e);
  const handleDragLeave = (e: ReactDragEvent) => onDragLeave(e);

  return (
    <div
      ref={rowRef}
      onContextMenu={onContextMenu}
      className="group mb-0.5 rounded-sm hover:bg-accent/60"
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
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          onClick={onSelect}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className={cn(
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs font-medium text-muted-foreground transition-colors',
            'hover:bg-accent/60 hover:text-foreground',
          )}
        >
          <ChevronRight
            className={cn(
              'h-3 w-3 shrink-0 text-muted-foreground/60 transition-transform',
              module.expanded && 'rotate-90 text-muted-foreground',
            )}
          />
          <span
            aria-hidden
            className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-amber-500/15 text-amber-500"
          >
            {module.expanded ? (
              <FolderOpen className="h-3 w-3" />
            ) : (
              <FolderClosed className="h-3 w-3" />
            )}
          </span>
          <InlineEdit
            value={module.name}
            onSave={onSaveEdit}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{module.name}</span>}
          />
          <span className="shrink-0 rounded bg-muted/60 px-1.5 font-mono text-[10px] tabular-nums text-muted-foreground">
            {endpointCount > 0 ? `${collectionCount}/${endpointCount}` : collectionCount}
          </span>
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
