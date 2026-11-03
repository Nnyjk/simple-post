import type { MouseEvent as ReactMouseEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import { MethodPill } from '@/components/ui/method-pill';
import type { DragContextValue } from '../tree-types';
import { TreeRow } from './TreeRow';

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
  dragContext: DragContextValue;
  /**
   * 被拖走的 id 集合。多选拖拽时是整个选区，否则只有这一行自己。默认只有这一行。
   */
  dragIds?: string[];
}

/**
 * 接口行。行容器、按钮、拖放回调与行内改名都在 `TreeRow` 里；这一层只留下接口特有的
 * 那几样东西：method 药丸、「已选」徽标，以及「打开着的那个请求」的强调样式。
 */
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
  dragContext,
  dragIds,
}: EndpointRowProps) {
  return (
    <TreeRow
      kind="endpoint"
      id={endpoint.id}
      name={endpoint.name}
      isEditing={isEditing}
      leading={
        // HTTP method 是这棵树保留的唯一图标：它是此处唯一携带该行尚未展示之数据的标记。
        // 原本在它前面的通用接口图标块重复了行类型本身，还把真正用来扫视的 method 又往右
        // 挤了一格。
        <MethodPill
          method={detail.method}
          size="xs"
          variant="solid"
          title={`HTTP ${detail.method}`}
        />
      }
      badge={
        multiSelect &&
        selected && (
          <span className="shrink-0 rounded bg-primary/20 px-1.5 text-[11px] font-medium text-primary">
            已选
          </span>
        )
      }
      onSelect={onClick}
      onStartEdit={onStartEdit}
      onSaveEdit={onSaveEdit}
      onCancelEdit={onCancelEdit}
      onContextMenu={onContextMenu}
      dragContext={dragContext}
      dragIds={dragIds}
      dataRow
      // `active` 是「标签条里打开着的那个请求」；`selected` 是「为批量操作勾上的」。两者
      // 是不同的事，故读起来也不同：active 得到强调条，selected 得到填充与「已选」徽标。
      //
      // 中性填充 + 3px 前导条是两家参照应用共同收敛到的写法，也让饱和强调色只留给那根条。
      outerClassName={cn(
        active && 'left-accent bg-surface-select',
        selected && !active && 'bg-surface-select',
      )}
      buttonClassName={cn(!active && !selected && 'hover:bg-overlay-hover')}
    />
  );
}
