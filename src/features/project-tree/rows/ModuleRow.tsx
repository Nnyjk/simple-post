import type { MouseEvent as ReactMouseEvent } from 'react';
import { ChevronRight } from 'lucide-react';
import type { ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import type { DragContextValue } from '../tree-types';
import { TreeRow } from './TreeRow';

export interface ModuleRowProps {
  module: ResourceNode;
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
 * 模块行。行容器、按钮、拖放回调与行内改名都在 `TreeRow` 里；这一层只留下模块特有的
 * 那几样东西：展开箭头、名字的 `font-medium`，以及行尾的「新建」按钮。
 */
export function ModuleRow({
  module,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onPlusClick,
  dragContext,
}: ModuleRowProps) {
  return (
    <TreeRow
      kind="module"
      id={module.id}
      name={module.name}
      isEditing={isEditing}
      leading={
        <ChevronRight
          className={cn(
            'h-3 w-3 shrink-0 text-faint transition-transform',
            module.expanded && 'rotate-90 text-muted-foreground',
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
      buttonClassName="font-medium hover:bg-overlay-hover"
    />
  );
}
