import {
  useState,
  type Dispatch,
  type MouseEvent as ReactMouseEvent,
  type SetStateAction,
} from 'react';
import {
  Copy,
  CornerDownRight,
  FilePlus,
  FolderPlus,
  Pencil,
  Terminal,
  Trash2,
} from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import type { DeleteTarget, EditTarget, TargetKind } from '../tree-types';
import type { ContextMenuItem } from '../TreeContextMenu';

export interface EditMenuBindings {
  setEditing: Dispatch<SetStateAction<EditTarget | null>>;
  openNewItemWithIntent: (opts: {
    type: 'project' | 'module' | 'collection' | 'endpoint';
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  openEndpointTab: (id: string) => void;
  setMenu: (m: { x: number; y: number; items: ContextMenuItem[] } | null) => void;
}

/**
 * Inline rename + context menu construction + the delete confirmation
 * flow. The `editing` value and its setter are owned by `ProjectTree`
 * (shared with `useNewItemFlow`, which needs the setter to drop a freshly
 * created row straight into rename); the store values read below are raw
 * references or `.find()` results.
 */
export function useEditMenu({
  setEditing,
  openNewItemWithIntent,
  openEndpointTab,
  setMenu,
}: EditMenuBindings) {
  // ---------- store actions ----------
  const setActiveEndpoint = useAppStore((s) => s.setActiveEndpoint);
  const updateProject = useAppStore((s) => s.updateProject);
  const updateModule = useAppStore((s) => s.updateModule);
  const updateCollection = useAppStore((s) => s.updateCollection);
  const updateEndpoint = useAppStore((s) => s.updateEndpoint);
  const deleteProject = useAppStore((s) => s.deleteProject);
  const deleteModule = useAppStore((s) => s.deleteModule);
  const deleteCollection = useAppStore((s) => s.deleteCollection);
  const deleteEndpoint = useAppStore((s) => s.deleteEndpoint);
  const duplicateEndpoint = useAppStore((s) => s.duplicateEndpoint);
  const moveCollections = useAppStore((s) => s.moveCollections);

  // ---------- raw state (all raw references — safe for selectors) ----------
  const projects = useAppStore((s) => s.projects);
  const modules = useAppStore((s) => s.modules);
  const collections = useAppStore((s) => s.collections);
  const endpoints = useAppStore((s) => s.endpoints);

  const [toDelete, setToDelete] = useState<DeleteTarget | null>(null);

  // ---------- edit / context menu / delete ----------
  const startEdit = (kind: TargetKind, id: string) => setEditing({ kind, id });
  const cancelEdit = () => setEditing(null);
  const saveEdit = (kind: TargetKind, id: string, next: string) => {
    if (kind === 'project') updateProject(id, { name: next });
    else if (kind === 'module') updateModule(id, { name: next });
    else if (kind === 'collection') updateCollection(id, { name: next });
    else if (kind === 'endpoint') updateEndpoint(id, { name: next });
    setEditing(null);
  };

  const openMenu = (e: ReactMouseEvent, kind: TargetKind, id: string) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, items: buildMenuItems(kind, id) });
  };

  const getName = (kind: TargetKind, id: string): string => {
    if (kind === 'project') return projects.find((p) => p.id === id)?.name ?? '';
    if (kind === 'module') return modules.find((m) => m.id === id)?.name ?? '';
    if (kind === 'collection') return collections.find((c) => c.id === id)?.name ?? '';
    return endpoints.find((ep) => ep.id === id)?.name ?? '';
  };

  const openDeleteDialog = (kind: TargetKind, id: string, cascade?: string) => {
    setToDelete({ kind, id, name: getName(kind, id), cascade });
  };

  const handleDeleteConfirm = () => {
    if (!toDelete) return;
    if (toDelete.kind === 'project') deleteProject(toDelete.id);
    else if (toDelete.kind === 'module') deleteModule(toDelete.id);
    else if (toDelete.kind === 'collection') deleteCollection(toDelete.id);
    else deleteEndpoint(toDelete.id);
    setToDelete(null);
  };

  const buildMenuItems = (kind: TargetKind, id: string): ContextMenuItem[] => {
    if (kind === 'project') {
      const isOnly = projects.length <= 1;
      return [
        {
          kind: 'item',
          label: '新建模块',
          icon: <FolderPlus className="h-3.5 w-3.5" />,
          onClick: () => openNewItemWithIntent({ type: 'module' }),
        },
        { kind: 'divider' },
        {
          kind: 'item',
          label: '重命名',
          icon: <Pencil className="h-3.5 w-3.5" />,
          onClick: () => startEdit('project', id),
        },
        {
          kind: 'item',
          label: '删除',
          icon: <Trash2 className="h-3.5 w-3.5" />,
          danger: true,
          disabled: isOnly,
          onClick: () => openDeleteDialog('project', id),
        },
      ];
    }
    if (kind === 'module') {
      const mod = modules.find((m) => m.id === id);
      const modCols = mod
        ? collections
            .filter((c) => c.moduleId === id && c.parentCollectionId === null)
            .sort((a, b) => a.sortOrder - b.sortOrder)
        : [];
      const subCollectionIds = mod
        ? collections
            .filter((c) => c.moduleId === id && c.parentCollectionId !== null)
            .map((c) => c.id)
        : [];
      const epCount = endpoints.filter((e) =>
        [...modCols.map((c) => c.id), ...subCollectionIds].includes(e.collectionId),
      ).length;
      const cascade =
        modCols.length > 0 || subCollectionIds.length > 0 || epCount > 0
          ? `将同时删除 ${modCols.length + subCollectionIds.length} 个集合、${epCount} 个接口`
          : undefined;
      return [
        {
          kind: 'item',
          label: '新建集合',
          icon: <FolderPlus className="h-3.5 w-3.5" />,
          onClick: () => openNewItemWithIntent({ type: 'collection', parentModuleId: id }),
        },
        { kind: 'divider' },
        {
          kind: 'item',
          label: '重命名',
          icon: <Pencil className="h-3.5 w-3.5" />,
          onClick: () => startEdit('module', id),
        },
        {
          kind: 'item',
          label: '删除',
          icon: <Trash2 className="h-3.5 w-3.5" />,
          danger: true,
          onClick: () => openDeleteDialog('module', id, cascade),
        },
      ];
    }
    if (kind === 'collection') {
      const col = collections.find((c) => c.id === id);
      if (!col) return [];
      const isTopLevel = col.parentCollectionId === null;
      const parentName = isTopLevel
        ? (modules.find((m) => m.id === col.moduleId)?.name ?? '')
        : (collections.find((c) => c.id === col.parentCollectionId)?.name ?? '');
      const colEps = endpoints.filter((e) => e.collectionId === id);
      const subCols = collections.filter((c) => c.parentCollectionId === id);
      const totalEps =
        colEps.length +
        endpoints.filter((e) => subCols.some((sc) => sc.id === e.collectionId)).length;
      const cascade =
        colEps.length > 0 || subCols.length > 0
          ? `将同时删除 ${colEps.length} 个接口、${subCols.length} 个子集合${
              totalEps - colEps.length > 0
                ? `（含子集合下的 ${totalEps - colEps.length} 个接口）`
                : ''
            }`
          : undefined;
      return [
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
              parentCollectionId: id,
              parentModuleId: col.moduleId,
            }),
        },
        { kind: 'divider' },
        {
          kind: 'item',
          label: '重命名',
          icon: <Pencil className="h-3.5 w-3.5" />,
          onClick: () => startEdit('collection', id),
        },
        ...(isTopLevel
          ? []
          : [
              {
                kind: 'item' as const,
                label: `脱离到「${parentName}」根级`,
                icon: <CornerDownRight className="h-3.5 w-3.5" />,
                onClick: () => {
                  // Move to the top-level of the same module: parent
                  // becomes null, appended at the end. Useful when a
                  // collection is misplaced as a sub-collection and
                  // the user wants to pop it back out.
                  const targetSiblings = collections.filter(
                    (c) => c.moduleId === col.moduleId && c.parentCollectionId === null,
                  );
                  moveCollections([id], {
                    parentKind: 'module',
                    parentId: col.moduleId,
                    index: targetSiblings.length,
                  });
                },
              },
            ]),
        { kind: 'divider' },
        {
          kind: 'item',
          label: '删除',
          icon: <Trash2 className="h-3.5 w-3.5" />,
          danger: true,
          onClick: () => openDeleteDialog('collection', id, cascade),
          // unused but keeps type-check quiet about idx:
          // (idx was used by the old up/down; now we rely on drag)
        },
      ];
    }
    // endpoint
    const ep = endpoints.find((e) => e.id === id);
    const epCol = ep ? collections.find((c) => c.id === ep.collectionId) : null;
    return [
      {
        kind: 'item',
        label: '重命名',
        icon: <Pencil className="h-3.5 w-3.5" />,
        onClick: () => startEdit('endpoint', id),
      },
      {
        kind: 'item',
        label: '复制',
        icon: <Copy className="h-3.5 w-3.5" />,
        onClick: () => {
          const newId = duplicateEndpoint(id);
          if (newId) {
            setActiveEndpoint(newId);
            setEditing({ kind: 'endpoint', id: newId });
          }
        },
      },
      {
        kind: 'item',
        label: '复制为 cURL',
        icon: <Terminal className="h-3.5 w-3.5" />,
        disabled: true,
        onClick: () => {
          // Copy as cURL — uses endpointToCurl from src/features/http-tester/curl.ts.
        },
      },
      ...(epCol
        ? [
            {
              kind: 'divider' as const,
            },
            {
              kind: 'item' as const,
              label: '脱离当前集合',
              icon: <CornerDownRight className="h-3.5 w-3.5" />,
              onClick: () => {
                // No-op placeholder: an endpoint must live in a
                // collection, so the natural "detach" is to move it
                // to a different one. We delegate to the user via
                // drag — clicking just opens a hint.
                openEndpointTab(id);
              },
            },
          ]
        : []),
      { kind: 'divider' },
      {
        kind: 'item',
        label: '删除',
        icon: <Trash2 className="h-3.5 w-3.5" />,
        danger: true,
        onClick: () => openDeleteDialog('endpoint', id),
      },
    ];
  };

  return {
    toDelete,
    cancelDelete: () => setToDelete(null),
    startEdit,
    cancelEdit,
    saveEdit,
    openMenu,
    handleDeleteConfirm,
  };
}
