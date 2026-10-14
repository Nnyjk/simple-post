import { ChevronRight, Plus } from 'lucide-react';
import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import { Tooltip } from '@/components/ui/tooltip';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DropZone } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

export interface CollectionRowProps {
  collection: ResourceNode;
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

export function CollectionRow({
  collection,
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
}: CollectionRowProps) {
  return (
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
            // 名字用 `--foreground` —— 见 `ModuleRow.tsx` 的说明。
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs text-foreground transition-colors',
            'hover:bg-overlay-hover',
          )}
        >
          <ChevronRight
            className={cn(
              'h-3 w-3 shrink-0 text-faint transition-transform',
              collection.expanded && 'rotate-90 text-muted-foreground',
            )}
          />
          {/* 没有类型图标、也没有数量徽标 —— 见 `ModuleRow.tsx` 的说明。展开状态由那个
              箭头承担。 */}
          <InlineEdit
            value={collection.name}
            onSave={onSaveEdit}
            // 由树的 `editing` 目标受控，好让从对话框创建的集合直接进入重命名。见
            // `EndpointRow.tsx` 处的同一条说明。
            editing={isEditing}
            onEditingChange={(open) => {
              if (!open) onCancelEdit();
            }}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{collection.name}</span>}
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
