import { FileCode2 } from 'lucide-react';
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
        'group mb-0.5 rounded-sm',
        active ? 'bg-primary/15' : selected ? 'bg-primary/10' : 'hover:bg-accent/60',
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
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs transition-colors',
            active
              ? 'bg-primary/15 text-foreground shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.20)]'
              : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
          )}
        >
          <span
            aria-hidden
            className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-emerald-500/15 text-emerald-500"
          >
            <FileCode2 className="h-3 w-3" />
          </span>
          <MethodPill
            method={detail.method}
            size="xs"
            variant="solid"
            title={`HTTP ${detail.method}`}
          />
          <InlineEdit
            value={endpoint.name}
            onSave={onSaveEdit}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{endpoint.name}</span>}
          />
          {multiSelect && selected && (
            <span className="shrink-0 rounded bg-primary/20 px-1.5 text-[10px] font-medium text-primary">
              已选
            </span>
          )}
        </button>
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}
