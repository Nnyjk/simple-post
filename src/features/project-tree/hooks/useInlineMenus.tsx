import { CornerDownRight, FilePlus, FolderPlus } from 'lucide-react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { ContextMenuItem } from '../TreeContextMenu';
import type { NewDraftKind } from '@/stores/app-store';

export interface InlineMenuBindings {
  /** Every node of the active project, any type, any depth. */
  nodes: ResourceNode[];
  /**
   * Opens the create *dialog* for the kinds that still have one.
   * Endpoints are deliberately absent — they are created by
   * `createEndpointAt`, not by a form.
   */
  openNewItemWithIntent: (opts: {
    type: NewDraftKind;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  /**
   * Create an endpoint under `parentId` and drop it into inline rename.
   * Resolves once the row exists; the caller owns the rename target, so
   * it can be the tree row it just added.
   */
  createEndpointAt: (parentId: string) => Promise<void>;
  setMenu: (m: { x: number; y: number; items: ContextMenuItem[] } | null) => void;
}

/**
 * The inline "+" menus attached to the module / collection rows. Each
 * builds its items lazily at click time (the first module / collection
 * of the project can change between renders), so no derived arrays are
 * memoized here.
 *
 * There is no project-level "+" any more: the project used to be a row
 * in this tree, and that row owned the "create the first module of this
 * project" entry point. With the project gone from the tree, that entry
 * point is the 「新建模块」 row at the top of the list.
 */
export function useInlineMenus({
  nodes,
  openNewItemWithIntent,
  createEndpointAt,
  setMenu,
}: InlineMenuBindings) {
  const openModulePlusMenu = (moduleId: string) => (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const items: ContextMenuItem[] = [
      {
        kind: 'item',
        label: '新建集合',
        icon: <FolderPlus className="h-3.5 w-3.5" />,
        onClick: () => openNewItemWithIntent({ type: 'collection', parentModuleId: moduleId }),
      },
      {
        kind: 'item',
        label: '新建接口',
        icon: <FilePlus className="h-3.5 w-3.5" />,
        // The module *is* the parent — an endpoint may sit directly under
        // one — so this needs no collection to exist first and no dialog.
        onClick: () => {
          void createEndpointAt(moduleId);
        },
      },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const openCollectionPlusMenu = (collectionId: string) => (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const col = nodes.find((c) => c.id === collectionId && c.kind === 'collection');
    if (!col) return;
    // A collection's parent can be another collection, so the owning
    // module is an ancestor, not `col.parentId`. The dialog only uses it
    // to name where the row will land; the write target is the collection.
    const moduleId = owningModuleId(nodes, collectionId);
    const items: ContextMenuItem[] = [
      {
        kind: 'item',
        label: '新建子集合',
        icon: <CornerDownRight className="h-3.5 w-3.5" />,
        onClick: () =>
          openNewItemWithIntent({
            type: 'collection',
            parentModuleId: moduleId ?? '',
            parentCollectionId: col.id,
          }),
      },
      {
        kind: 'item',
        label: '新建接口',
        icon: <FilePlus className="h-3.5 w-3.5" />,
        onClick: () => {
          void createEndpointAt(col.id);
        },
      },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  return { openModulePlusMenu, openCollectionPlusMenu };
}

/**
 * Walk up `parentId` until a module row is found. Cycles are impossible
 * in a healthy tree, but the `seen` guard keeps a corrupted one from
 * spinning forever inside a click handler.
 */
function owningModuleId(nodes: ResourceNode[], collectionId: string): string | undefined {
  const seen = new Set<string>();
  let cur = nodes.find((n) => n.id === collectionId);
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    if (cur.kind === 'module') return cur.id;
    cur = cur.parentId ? nodes.find((n) => n.id === cur!.parentId) : undefined;
  }
  return undefined;
}
