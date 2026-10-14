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
    // 合并注记：合并时已应用 `wip/module-baseurls` 一侧对 `rowRef` 的删除 —— 声明已从文件
    // 移除，`src/` 下别处也不再出现 `rowRef`，故此行保留 `ref={rowRef}` 根本无法编译。这里
    // 取那一侧的结构，并保留审美那一侧的外层圆角；外层的 `hover:bg-accent/60` 去掉，以与
    // `CollectionRow` 一致 —— 后者把 hover 放在内层按钮上作 `bg-overlay-hover`。
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
            // 名字是这一行存在的理由，故用 `--foreground` 渲染。次级信息（那个箭头）才用弱化
            // 色阶 —— 这正是设三级文字的意义。
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
          {/* 这里没有类型图标。上方的展开箭头已经说明「这东西会展开」，而这棵树里的每种
              节点都是容器 —— 类型图标只是重复缩进与旁边的名字已经传达的内容。唯一携带行本身
              没有的信息的图标是接口上的 HTTP method，那个保留。

              右侧也没有数量徽标：它数的是箭头已经揭示出来的子项，而展开只需一次点击 —— 重复
              树自身结构的数字是噪声，不是信息。 */}
          <InlineEdit
            value={module.name}
            onSave={onSaveEdit}
            // 由树的 `editing` 目标受控，好让从对话框创建的模块直接进入重命名。见
            // `EndpointRow.tsx` 处的同一条说明。
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
