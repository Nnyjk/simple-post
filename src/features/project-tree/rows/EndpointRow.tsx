import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import { MethodPill } from '@/components/ui/method-pill';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DropZone } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

export interface EndpointRowProps {
  endpoint: ResourceNode;
  /** 请求配置 —— method pill 从这里读取。 */
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
        // `active` 是「标签条里打开着的那个请求」；`selected` 是「为批量操作勾上的」。两者
        // 是不同的事，故读起来也不同：active 得到强调条，selected 得到填充与「已选」徽标。
        //
        // 中性填充 + 3px 前导条是两家参照应用共同收敛到的写法，也让饱和强调色只留给那根条。
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
            // 与 ModuleRow 同一条规则：名字承担这一行，method pill 承担数据。两者都不弱化。
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs text-foreground transition-colors',
            !active && !selected && 'hover:bg-overlay-hover',
          )}
        >
          {/* HTTP method 是这棵树保留的唯一图标：它是此处唯一携带该行尚未展示之数据的标记。
              原本在它前面的通用接口图标块重复了行类型本身，还把真正用来扫视的 method 又往右
              挤了一格。 */}
          <MethodPill
            method={detail.method}
            size="xs"
            variant="solid"
            title={`HTTP ${detail.method}`}
          />
          <InlineEdit
            value={endpoint.name}
            onSave={onSaveEdit}
            // 由树的 `editing` 目标受控，而非本地的双击：从「+」菜单创建接口必须能把新行直接
            // 丢进重命名，而那个信号是树状态传来的。本行只响应 `false`（用户完成或取消）这
            // 一个跃迁 —— 打开是树的事，本行自己的双击通过上面的 `onStartEdit` 抵达它。
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
