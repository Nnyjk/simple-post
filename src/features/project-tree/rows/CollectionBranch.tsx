import type { MouseEvent as ReactMouseEvent } from 'react';
import type {
  CollectionBranch as CollectionBranchNode,
  DragContextValue,
  TargetKind,
} from '../tree-types';
import { CollectionRow } from './CollectionRow';
import { EndpointRow } from './EndpointRow';
import { TreeChildren } from './TreeChildren';

export interface CollectionBranchProps {
  branches: CollectionBranchNode[];
  depth: number;
  editing: { kind: TargetKind; id: string } | null;
  onSelectCollection: (id: string) => void;
  onStartEditCollection: (id: string) => void;
  onSaveEditCollection: (id: string, name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent, kind: TargetKind, id: string) => void;
  onPlusClick: (collectionId: string) => (e: ReactMouseEvent) => void;
  selectedEndpointIds: Set<string>;
  activeEndpointId: string | null;
  onEndpointClick: (id: string, e: ReactMouseEvent) => void;
  onEndpointStartEdit: (id: string) => void;
  onEndpointContextMenu: (e: ReactMouseEvent, id: string) => void;
  onEndpointSaveEdit: (id: string, name: string) => void;
  dragContext: DragContextValue;
}

/**
 * Recursive renderer for a list of sibling collections. Each collection
 * renders its own row + its children (endpoints + sub-collections) when
 * expanded. Recursion bottoms out at the deepest collection.
 */
export function CollectionBranch(props: CollectionBranchProps) {
  const {
    branches,
    depth,
    editing,
    onSelectCollection,
    onStartEditCollection,
    onSaveEditCollection,
    onCancelEdit,
    onContextMenu,
    onPlusClick,
    selectedEndpointIds,
    activeEndpointId,
    onEndpointClick,
    onEndpointStartEdit,
    onEndpointContextMenu,
    onEndpointSaveEdit,
    dragContext,
  } = props;
  return (
    <div
      className="mt-0.5 border-l border-border/60 pl-1"
      style={{ marginLeft: depth === 1 ? '0.5rem' : 0 }}
    >
      {branches.map(
        ({ collection, endpoints: colEndpoints, endpointDetails, children: subCols }) => (
          <div key={collection.id} className="mb-0.5" data-tree-row>
            <CollectionRow
              collection={collection}
              endpointCount={colEndpoints.length}
              isEditing={editing?.kind === 'collection' && editing.id === collection.id}
              onSelect={() => onSelectCollection(collection.id)}
              onStartEdit={() => onStartEditCollection(collection.id)}
              onSaveEdit={(name) => onSaveEditCollection(collection.id, name)}
              onCancelEdit={onCancelEdit}
              onContextMenu={(e) => onContextMenu(e, 'collection', collection.id)}
              onPlusClick={onPlusClick(collection.id)}
              onDragStart={(e) =>
                dragContext.setDataTransfer(e, { kind: 'collection', ids: [collection.id] })
              }
              onDragEnd={dragContext.handleDragEnd}
              onDragOver={dragContext.makeDragOverHandler(
                { kind: 'collection', id: collection.id },
                null,
              )}
              onDragLeave={dragContext.makeDragLeaveHandler({
                kind: 'collection',
                id: collection.id,
              })}
              onDrop={dragContext.makeDropHandler({ kind: 'collection', id: collection.id }, null)}
              dropHint={
                dragContext.dropHint && dragContext.dropHint.key === `collection:${collection.id}`
                  ? dragContext.dropHint.zone
                  : null
              }
            />
            {(colEndpoints.length > 0 || subCols.length > 0) && (
              <TreeChildren expanded={!!collection.expanded}>
                {colEndpoints.length > 0 && (
                  <div className="ml-3 border-l border-border/60 pl-1">
                    {colEndpoints.map((ep) => {
                      // An endpoint with no detail row is a tree the
                      // backend would refuse to create, so a missing one
                      // here means the two tables disagree. Skip the row
                      // rather than render a method pill off `undefined`.
                      const epDetail = endpointDetails[ep.id];
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
                            // When the user starts dragging a single
                            // endpoint that is part of a multi-selection,
                            // drag the whole selection. Otherwise drag
                            // just the one row.
                            const ids =
                              selectedEndpointIds.has(ep.id) && selectedEndpointIds.size > 1
                                ? Array.from(selectedEndpointIds)
                                : [ep.id];
                            dragContext.setDataTransfer(e, { kind: 'endpoint', ids });
                          }}
                          onDragEnd={dragContext.handleDragEnd}
                          onDragOver={dragContext.makeDragOverHandler(
                            { kind: 'endpoint', id: ep.id },
                            null,
                          )}
                          onDragLeave={dragContext.makeDragLeaveHandler({
                            kind: 'endpoint',
                            id: ep.id,
                          })}
                          onDrop={dragContext.makeDropHandler(
                            { kind: 'endpoint', id: ep.id },
                            null,
                          )}
                          dropHint={
                            dragContext.dropHint && dragContext.dropHint.key === `endpoint:${ep.id}`
                              ? dragContext.dropHint.zone
                              : null
                          }
                        />
                      );
                    })}
                  </div>
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
