import type { MouseEvent as ReactMouseEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { DragContextValue } from '../tree-types';
import { TreeRow } from './TreeRow';

export interface CollectionRowProps {
  collection: ResourceNode;
  isEditing: boolean;
  onSelect: () => void;
  /** 点展开箭头时调用 —— 只切展开状态，不打开工作区。 */
  onToggle: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
  dragContext: DragContextValue;
}

/**
 * 集合行。行容器、按钮、拖放回调与行内改名都在 `TreeRow` 里；这一层只留下集合特有的
 * 那几样东西：行尾的「新建」按钮。
 *
 * 展开箭头由 `TreeRow` 渲染：它是一个独立的小按钮，只切换展开状态，与打开工作区分开。
 */
export function CollectionRow({
  collection,
  isEditing,
  onSelect,
  onToggle,
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
      toggle={{ expanded: collection.expanded === true, onToggle }}
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
