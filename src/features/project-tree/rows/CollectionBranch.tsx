import type { MouseEvent as ReactMouseEvent } from 'react';
import type {
  CollectionBranch as CollectionBranchNode,
  DragContextValue,
  TargetKind,
} from '../tree-types';
import { CollectionRow } from './CollectionRow';
import { EndpointList } from './EndpointList';
import { TreeChildren } from './TreeChildren';

export interface CollectionBranchProps {
  branches: CollectionBranchNode[];
  depth: number;
  onSelectCollection: (id: string) => void;
  /** 点展开箭头、或双击行时调用 —— 只切展开状态，不打开工作区。 */
  onToggleCollection: (id: string) => void;
  onContextMenu: (e: ReactMouseEvent, kind: TargetKind, id: string) => void;
  onPlusClick: (collectionId: string) => (e: ReactMouseEvent) => void;
  selectedEndpointIds: Set<string>;
  activeEndpointId: string | null;
  onEndpointClick: (id: string, e: ReactMouseEvent) => void;
  onEndpointContextMenu: (e: ReactMouseEvent, id: string) => void;
  dragContext: DragContextValue;
}

/**
 * 一组兄弟集合的递归渲染器。每个集合在展开时渲染自己的行及其子级（接口 + 子集合）。
 * 递归在最深的集合处到底。
 */
export function CollectionBranch(props: CollectionBranchProps) {
  const {
    branches,
    depth,
    onSelectCollection,
    onToggleCollection,
    onContextMenu,
    onPlusClick,
    selectedEndpointIds,
    activeEndpointId,
    onEndpointClick,
    onEndpointContextMenu,
    dragContext,
  } = props;
  return (
    <div
      className="mt-0.5 border-l border-border/60 pl-1"
      style={{ marginLeft: depth === 1 ? '0.5rem' : 0 }}
    >
      {branches.map(
        ({ collection, endpoints: colEndpoints, endpointDetails, children: subCols }) => (
          // 标记挂在行本身而不是这个包装上：包装里还有嵌套的 `TreeChildren`，父级展开时
          // 若整体淡入，里面已经展开的行会跟着再淡一次。
          <div key={collection.id} className="mb-0.5">
            <CollectionRow
              collection={collection}
              onSelect={() => onSelectCollection(collection.id)}
              onToggle={() => onToggleCollection(collection.id)}
              onContextMenu={(e) => onContextMenu(e, 'collection', collection.id)}
              onPlusClick={onPlusClick(collection.id)}
              dragContext={dragContext}
              dataRow
            />
            {(colEndpoints.length > 0 || subCols.length > 0) && (
              <TreeChildren expanded={!!collection.expanded}>
                {colEndpoints.length > 0 && (
                  <EndpointList
                    endpoints={colEndpoints}
                    details={endpointDetails}
                    selectedEndpointIds={selectedEndpointIds}
                    activeEndpointId={activeEndpointId}
                    onEndpointClick={onEndpointClick}
                    onEndpointContextMenu={onEndpointContextMenu}
                    dragContext={dragContext}
                  />
                )}
                {subCols.length > 0 && (
                  <CollectionBranch {...props} branches={subCols} depth={depth + 1} />
                )}
              </TreeChildren>
            )}
          </div>
        ),
      )}
    </div>
  );
}
