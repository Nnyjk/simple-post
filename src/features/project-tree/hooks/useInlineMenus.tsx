import { CornerDownRight, FilePlus, FolderPlus } from 'lucide-react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import type { Collection, Module } from '@/types/domain';
import type { ContextMenuItem } from '../TreeContextMenu';
import type { NewType } from '@/stores/app-store';

export interface InlineMenuBindings {
  collections: Collection[];
  projectModules: Module[];
  openNewItemWithIntent: (opts: {
    type: NewType;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  setMenu: (m: { x: number; y: number; items: ContextMenuItem[] } | null) => void;
}

/**
 * The inline "+" menus attached to the project / module / collection
 * rows. Each builds its items lazily at click time (the first module /
 * collection of the project can change between renders), so no derived
 * arrays are memoized here.
 */
export function useInlineMenus({
  collections,
  projectModules,
  openNewItemWithIntent,
  setMenu,
}: InlineMenuBindings) {
  const openProjectPlusMenu = (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const firstMod = projectModules[0];
    const firstColSorted = firstMod
      ? collections
          .filter((c) => c.moduleId === firstMod.id && c.parentCollectionId === null)
          .sort((a, b) => a.sortOrder - b.sortOrder)[0]
      : null;
    const items: ContextMenuItem[] = [
      {
        kind: 'item',
        label: '新建模块',
        icon: <FolderPlus className="h-3.5 w-3.5" />,
        onClick: () => openNewItemWithIntent({ type: 'module' }),
      },
    ];
    if (firstMod) {
      items.push({
        kind: 'item',
        label: '新建集合',
        icon: <FolderPlus className="h-3.5 w-3.5" />,
        onClick: () => openNewItemWithIntent({ type: 'collection', parentModuleId: firstMod.id }),
      });
    }
    if (firstMod && firstColSorted) {
      items.push({
        kind: 'item',
        label: '新建接口',
        icon: <FilePlus className="h-3.5 w-3.5" />,
        onClick: () =>
          openNewItemWithIntent({
            type: 'endpoint',
            parentModuleId: firstMod.id,
            parentCollectionId: firstColSorted.id,
          }),
      });
    }
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

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
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const openCollectionPlusMenu = (collectionId: string) => (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const col = collections.find((c) => c.id === collectionId);
    if (!col) return;
    const items: ContextMenuItem[] = [
      {
        kind: 'item',
        label: '新建子集合',
        icon: <CornerDownRight className="h-3.5 w-3.5" />,
        onClick: () =>
          openNewItemWithIntent({
            type: 'collection',
            parentModuleId: col.moduleId,
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
            parentModuleId: col.moduleId,
            parentCollectionId: col.id,
          }),
      },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  return { openProjectPlusMenu, openModulePlusMenu, openCollectionPlusMenu };
}
