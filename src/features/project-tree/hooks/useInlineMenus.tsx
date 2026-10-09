import { CornerDownRight, FilePlus, FolderPlus } from 'lucide-react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { ContextMenuItem } from '../TreeContextMenu';
import type { NewType } from '@/stores/app-store';

export interface InlineMenuBindings {
  /** Every node of the active project, any type, any depth. */
  nodes: ResourceNode[];
  openNewItemWithIntent: (opts: {
    type: NewType;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
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
 * point is the empty state in `ProjectTree` plus the panel's own "新建"
 * button, both of which reach the same modal.
 */
export function useInlineMenus({ nodes, openNewItemWithIntent, setMenu }: InlineMenuBindings) {
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
      // A module takes an endpoint directly, so this needs no collection
      // to exist first — passing only `parentModuleId` is the whole
      // "直接放在模块下" intent.
      {
        kind: 'item',
        label: '新建接口',
        icon: <FilePlus className="h-3.5 w-3.5" />,
        onClick: () => openNewItemWithIntent({ type: 'endpoint', parentModuleId: moduleId }),
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
    // module is an ancestor, not `col.parentId`. The modal only uses
    // `parentModuleId` to preselect the "所属模块" dropdown — the actual
    // write target is `parentCollectionId`.
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
        onClick: () =>
          openNewItemWithIntent({
            type: 'endpoint',
            parentModuleId: moduleId ?? '',
            parentCollectionId: col.id,
          }),
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
