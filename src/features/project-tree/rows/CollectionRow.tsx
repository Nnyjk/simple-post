import type { MouseEvent as ReactMouseEvent } from 'react';
import { ChevronRight } from 'lucide-react';
import type { ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import type { DragContextValue } from '../tree-types';
import { TreeRow } from './TreeRow';

export interface CollectionRowProps {
  collection: ResourceNode;
  isEditing: boolean;
  onSelect: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
  dragContext: DragContextValue;
}

/**
 * 集合行。行容器、按钮、拖放回调与行内改名都在 `TreeRow` 里；这一层只留下集合特有的
 * 那几样东西：展开箭头，以及行尾的「新建」按钮。
 */
export function CollectionRow({
  collection,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onPlusClick,
  dragContext,
}: CollectionRowProps) {
  return (
    <TreeRow
      kind="collection"
      id={collection.id}
      name={collection.name}
      isEditing={isEditing}
      leading={
        <ChevronRight
          className={cn(
            'h-3 w-3 shrink-0 text-faint transition-transform',
            collection.expanded && 'rotate-90 text-muted-foreground',
          )}
        />
      }
      onPlusClick={onPlusClick}
      onSelect={onSelect}
      onStartEdit={onStartEdit}
      onSaveEdit={onSaveEdit}
      onCancelEdit={onCancelEdit}
      onContextMenu={onContextMenu}
      dragContext={dragContext}
      buttonClassName="hover:bg-overlay-hover"
    />
  );
}
