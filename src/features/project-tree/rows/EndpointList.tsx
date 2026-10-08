import type { MouseEvent as ReactMouseEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import type { DragContextValue, TargetKind } from '../tree-types';
import { EndpointRow } from './EndpointRow';

export interface EndpointListProps {
  endpoints: ResourceNode[];
  /** Detail rows for `endpoints`, already narrowed to the same set. */
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
 * One container's own endpoint rows, wired for selection, rename,
 * context menu and drag-and-drop.
 *
 * Both kinds of container need this: a collection holds endpoints, and a
 * module holds them too when they need no collection of their own. Keeping
 * the wiring here rather than in each caller means the multi-select drag
 * gesture and the missing-detail guard cannot drift between the two
 * levels.
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
        // An endpoint with no detail row is a tree the backend would
        // refuse to create, so a missing one here means the two tables
        // disagree. Skip the row rather than render a method pill off
        // `undefined`.
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
              // When the user starts dragging a single endpoint that is
              // part of a multi-selection, drag the whole selection.
              // Otherwise drag just the one row.
              const ids =
                selectedEndpointIds.has(ep.id) && selectedEndpointIds.size > 1
                  ? Array.from(selectedEndpointIds)
                  : [ep.id];
              dragContext.setDataTransfer(e, { kind: 'endpoint', ids });
            }}
            onDragEnd={dragContext.handleDragEnd}
            onDragOver={dragContext.makeDragOverHandler({ kind: 'endpoint', id: ep.id }, null)}
            onDragLeave={dragContext.makeDragLeaveHandler({ kind: 'endpoint', id: ep.id })}
            onDrop={dragContext.makeDropHandler({ kind: 'endpoint', id: ep.id }, null)}
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
