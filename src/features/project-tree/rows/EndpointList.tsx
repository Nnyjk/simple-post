import type { MouseEvent as ReactMouseEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import type { DragContextValue, TargetKind } from '../tree-types';
import { EndpointRow } from './EndpointRow';

export interface EndpointListProps {
  endpoints: ResourceNode[];
  /** `endpoints` 对应的详情行，已收窄到同一集合。 */
  details: Record<string, EndpointDetail>;
  editing: { kind: TargetKind; id: string } | null;
  selectedEndpointIds: Set<string>;
  activeEndpointId: string | null;
  onEndpointClick: (id: string, e: ReactMouseEvent) => void;
  onEndpointStartEdit: (id: string) => void;
  onEndpointContextMenu: (e: ReactMouseEvent, id: string) => void;
  onEndpointSaveEdit: (id: string, name: string) => void;
  onCancelEdit: () => void;
  dragContext: DragContextValue;
}

/**
 * 单个容器自己的接口行，接好选中、重命名、右键菜单与拖放。
 *
 * 两类容器都需要它：集合装接口，模块在接口不需要自己的集合时也装。把接线放在这里而不是
 * 各调用方，好让多选拖动手势与「缺详情行」的守卫不会在两个层级之间走样。
 */
export function EndpointList({
  endpoints,
  details,
  editing,
  selectedEndpointIds,
  activeEndpointId,
  onEndpointClick,
  onEndpointStartEdit,
  onEndpointContextMenu,
  onEndpointSaveEdit,
  onCancelEdit,
  dragContext,
}: EndpointListProps) {
  return (
    <div className="ml-3 border-l border-border/60 pl-1">
      {endpoints.map((ep) => {
        // 没有详情行的接口，后端根本不会创建这样的树，故这里缺一行意味着两张表不一致。
        // `EndpointList` 跳过该行，而不是拿 `undefined` 去渲染 method pill。
        const epDetail = details[ep.id];
        if (!epDetail) return null;
        return (
          <EndpointRow
            key={ep.id}
            endpoint={ep}
            detail={epDetail}
            active={ep.id === activeEndpointId}
            selected={selectedEndpointIds.has(ep.id)}
            multiSelect={selectedEndpointIds.size > 1}
            isEditing={editing?.kind === 'endpoint' && editing.id === ep.id}
            onClick={(e) => onEndpointClick(ep.id, e)}
            onStartEdit={() => onEndpointStartEdit(ep.id)}
            onSaveEdit={(name) => onEndpointSaveEdit(ep.id, name)}
            onCancelEdit={onCancelEdit}
            onContextMenu={(e) => onEndpointContextMenu(e, ep.id)}
            onDragStart={(e) => {
              // 拖拽一个属于多选集合的接口时，拖的是整个选区；否则只拖这一行。
              const ids =
                selectedEndpointIds.has(ep.id) && selectedEndpointIds.size > 1
                  ? Array.from(selectedEndpointIds)
                  : [ep.id];
              dragContext.setDataTransfer(e, { kind: 'endpoint', ids });
            }}
            onDragEnd={dragContext.handleDragEnd}
            onDragOver={dragContext.makeDragOverHandler({ kind: 'endpoint', id: ep.id })}
            onDragLeave={dragContext.makeDragLeaveHandler({ kind: 'endpoint', id: ep.id })}
            onDrop={dragContext.makeDropHandler({ kind: 'endpoint', id: ep.id })}
            dropHint={
              dragContext.dropHint && dragContext.dropHint.key === `endpoint:${ep.id}`
                ? dragContext.dropHint.zone
                : null
            }
          />
        );
      })}
    </div>
  );
}
