import type {
  DragEvent as ReactDragEvent,
  MouseEvent as ReactMouseEvent,
  ReactNode,
} from 'react';
import { ChevronRight, Plus } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { Tooltip } from '@/components/ui/tooltip';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DragContextValue, TreeNodeKind } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

export interface TreeRowProps {
  /**
   * 这一行的类型。它决定这一行在拖放里扮演什么角色 —— 模块、集合与接口的落点规则
   * 各不相同，那些规则住在 `../drop-resolve.ts`，不在这一层。
   */
  kind: TreeNodeKind;
  /** 行 id。 */
  id: string;
  /** 行上显示的名字。 */
  name: string;
  /** 这一行是否正在被重命名。 */
  isEditing: boolean;
  /**
   * 展开 / 收起的箭头。给了这一项，行左侧就多一个**独立的小按钮**。
   *
   * 两件事刻意分开：行按钮只打开工作区，箭头只切换展开状态。曾经两者绑在同一个点击上，
   * 于是点箭头会顺手打开工作区、点名字会顺手把刚展开的分支收起来 —— 两个后果都与用户
   * 当下的意图相反。
   */
  toggle?: {
    /** 这一行当前是否展开。 */
    expanded: boolean;
    /** 点箭头时调用。 */
    onToggle: () => void;
  };
  /**
   * 前导内容：接口行的 method 药丸。
   *
   * 药丸留在名称方块**里面** —— 它与名称是一体的（药丸在前，名称在后）。药丸若排在
   * 名称方块外面，接口行的名称便会落到上级名称的右边。
   */
  leading?: ReactNode;
  /** 按钮内的后置内容：接口行的「已选」徽标。 */
  badge?: ReactNode;
  /** 行尾的「新建」按钮组。模块与集合有，接口没有。 */
  onPlusClick?: (e: ReactMouseEvent) => void;
  onSelect: (e: ReactMouseEvent) => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  /** 拖放接线。行自己按 `kind` 与 `id` 接上回调，落点提示也自己从里面读。 */
  dragContext: DragContextValue;
  /** 被拖走的 id 集合。默认只有这一行自己。 */
  dragIds?: string[];
  /** 行容器的额外 class。 */
  outerClassName?: string;
  /** 行按钮的额外 class。 */
  buttonClassName?: string;
  /** 这一行是否带 `data-tree-row` 标记 —— 展开动画按它逐行淡入。 */
  dataRow?: boolean;
}

/**
 * 树里三种行的共同外壳：行容器、行按钮、拖放回调、落点提示与行内改名。
 *
 * 三种行原本各写一遍这整套接线。改动落在三处时（三次「重命名时那一行退出拖拽」就是
 * 这么来的），三处都得改，而漏掉一处不会有任何报错。故接线收进这一层，三种行各自只
 * 留下真正不同的部分：前置图标、尾部徽标、行尾的「新建」按钮，以及各自的 class。
 *
 * 展开箭头是**另一个**按钮，与行按钮平级：它只切换展开状态，不打开工作区。
 */
export function TreeRow({
  kind,
  id,
  name,
  isEditing,
  toggle,
  leading,
  badge,
  onPlusClick,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  dragContext,
  dragIds,
  outerClassName,
  buttonClassName,
  dataRow,
}: TreeRowProps) {
  const dropHint =
    dragContext.dropHint && dragContext.dropHint.key === `${kind}:${id}`
      ? dragContext.dropHint.zone
      : null;

  /** 行按钮是这一行的拖拽起点 —— 箭头不参与。 */
  const startDrag = (e: ReactDragEvent) => {
    dragContext.setDataTransfer(e, { kind, ids: dragIds ?? [id] });
  };

  return (
    <div
      data-tree-row={dataRow ? '' : undefined}
      onContextMenu={onContextMenu}
      className={cn('group mb-0.5 rounded-md', outerClassName)}
    >
      <div
        className={cn(
          'relative flex items-center gap-0.5 rounded-sm transition-colors',
          dropHint === 'into' && 'bg-primary/15 ring-1 ring-primary/40',
        )}
      >
        {toggle && (
          <button
            type="button"
            // 箭头不发起拖拽：它是展开 / 收起的控件，拖它既没有意义，也会让人以为
            // 这一行能被拖走。放置仍然认它 —— 把别的节点拖到箭头那一小条上，与拖到
            // 名称上是同一件事，故 drop 那几个回调留在它上面。
            onDragOver={dragContext.makeDragOverHandler({ kind, id })}
            onDragLeave={dragContext.makeDragLeaveHandler({ kind, id })}
            onDrop={dragContext.makeDropHandler({ kind, id })}
            onClick={toggle.onToggle}
            aria-expanded={toggle.expanded}
            aria-label={toggle.expanded ? '收起' : '展开'}
            title={toggle.expanded ? '收起' : '展开'}
            data-testid="tree-toggle-btn"
            className={cn(
              'flex shrink-0 items-center justify-center rounded-sm px-1 text-faint transition-colors hover:bg-overlay-hover hover:text-foreground',
              focusRing,
            )}
          >
            <ChevronRight
              className={cn(
                'h-3 w-3 transition-transform',
                toggle.expanded && 'rotate-90 text-muted-foreground',
              )}
            />
          </button>
        )}
        <button
          type="button"
          // 正在改名的那一行不参与拖拽。理由见 `EndpointRow.tsx` 的同一处：重命名输入框就在
          // 这个 button 里面，而 `draggable` 的祖先会把「在输入框里框选文字」的手势读成
          // 「拖这一行」。
          draggable={!isEditing}
          onDragStart={startDrag}
          onDragEnd={dragContext.handleDragEnd}
          onDragOver={dragContext.makeDragOverHandler({ kind, id })}
          onDragLeave={dragContext.makeDragLeaveHandler({ kind, id })}
          onDrop={dragContext.makeDropHandler({ kind, id })}
          onClick={onSelect}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className={cn(
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs text-foreground transition-colors',
            buttonClassName,
          )}
        >
          {leading}
          <InlineEdit
            value={name}
            onSave={onSaveEdit}
            // 由树的 `editing` 目标受控，而非本地的双击：从「+」菜单创建的行必须能把新行
            // 直接丢进重命名，而那个信号是树状态传来的。本行只响应 `false`（用户完成或取消）
            // 这一个跃迁 —— 打开是树的事，行自己的双击通过上面的 `onStartEdit` 抵达它。
            editing={isEditing}
            onEditingChange={(open) => {
              if (!open) onCancelEdit();
            }}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{name}</span>}
          />
          {badge}
        </button>
        {onPlusClick && (
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
        )}
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}
