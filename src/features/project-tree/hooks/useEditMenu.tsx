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
import type { ResourceNode } from '@/types/domain';
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
  const updateNode = useAppStore((s) => s.updateNode);
  const deleteProject = useAppStore((s) => s.deleteProject);
  const deleteNode = useAppStore((s) => s.deleteNode);
  const duplicateEndpoint = useAppStore((s) => s.duplicateEndpoint);
  const moveNodes = useAppStore((s) => s.moveNodes);

  // ---------- raw state (all raw references — safe for selectors) ----------
  const projects = useAppStore((s) => s.projects);
  const nodes = useAppStore((s) => s.nodes);

  const [toDelete, setToDelete] = useState<DeleteTarget | null>(null);

  /**
   * A node and every descendant beneath it, BFS on `parentId`.
   *
   * Used only to phrase the delete confirmation ("将同时删除 N 个
   * 集合、M 个接口"), so it is a count over the visible tree rather
   * than a reason to trust the client's view of the cascade — the
   * database cascades regardless.
   */
  const subtreeOf = (rootId: string): ResourceNode[] => {
    const childIndex = new Map<string, ResourceNode[]>();
    for (const n of nodes) {
      if (!n.parentId) continue;
      const bucket = childIndex.get(n.parentId);
      if (bucket) bucket.push(n);
      else childIndex.set(n.parentId, [n]);
    }
    const out: ResourceNode[] = [];
    const seen = new Set<string>();
    const queue = [rootId];
    while (queue.length > 0) {
      const id = queue.shift()!;
      if (seen.has(id)) continue;
      seen.add(id);
      const self = nodes.find((n) => n.id === id);
      if (self) out.push(self);
      for (const k of childIndex.get(id) ?? []) queue.push(k.id);
    }
    return out;
  };

  // ---------- edit / context menu / delete ----------
  const startEdit = (kind: TargetKind, id: string) => setEditing({ kind, id });
  const cancelEdit = () => setEditing(null);
  const saveEdit = (kind: TargetKind, id: string, next: string) => {
    if (kind === 'project') updateProject(id, { name: next });
    else updateNode(id, { name: next });
    setEditing(null);
  };

  const openMenu = (e: ReactMouseEvent, kind: TargetKind, id: string) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, items: buildMenuItems(kind, id) });
  };

  const getName = (kind: TargetKind, id: string): string => {
    if (kind === 'project') return projects.find((p) => p.id === id)?.name ?? '';
    return nodes.find((n) => n.id === id)?.name ?? '';
  };

  const openDeleteDialog = (kind: TargetKind, id: string, cascade?: string) => {
    setToDelete({ kind, id, name: getName(kind, id), cascade });
  };

  const handleDeleteConfirm = () => {
    if (!toDelete) return;
    if (toDelete.kind === 'project') deleteProject(toDelete.id);
    else deleteNode(toDelete.id);
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
      const subtree = subtreeOf(id);
      const subCols = subtree.filter((n) => n.kind === 'collection');
      const epCount = subtree.filter((n) => n.kind === 'endpoint').length;
      const cascade =
        subCols.length > 0 || epCount > 0
          ? `将同时删除 ${subCols.length} 个集合、${epCount} 个接口`
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
      const col = nodes.find((c) => c.id === id);
      if (!col) return [];
      // A top-level collection's parent is its module; a nested one's is
      // the collection above it. `parentId` says which, with no flag to
      // keep in sync.
      const isTopLevel = col.parentId
        ? nodes.find((n) => n.id === col.parentId)?.kind === 'module'
        : false;
      const parentName = col.parentId ? (nodes.find((n) => n.id === col.parentId)?.name ?? '') : '';
      const ownEps = nodes.filter((n) => n.kind === 'endpoint' && n.parentId === id);
      const subCols = nodes.filter((n) => n.kind === 'collection' && n.parentId === id);
      const subEpCount = subtreeOf(id).filter(
        (n) => n.kind === 'endpoint' && n.parentId !== id,
      ).length;
      const cascade =
        ownEps.length > 0 || subCols.length > 0
          ? `将同时删除 ${ownEps.length} 个接口、${subCols.length} 个子集合${
              subEpCount > 0 ? `（含子集合下的 ${subEpCount} 个接口）` : ''
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
            }),
        },
        { kind: 'divider' },
        {
          kind: 'item',
          label: '重命名',
          icon: <Pencil className="h-3.5 w-3.5" />,
          onClick: () => startEdit('collection', id),
        },
        ...(isTopLevel || !col.parentId
          ? []
          : [
              {
                kind: 'item' as const,
                label: `脱离到「${parentName}」根级`,
                icon: <CornerDownRight className="h-3.5 w-3.5" />,
                onClick: () => {
                  // Pop this collection back out to the top level of
                  // whatever module the parent lives in. Useful when a
                  // collection was nested by mistake.
                  //
                  // The destination is the parent's own parent — under
                  // the unified tree, "the root level of this module"
                  // is just "wherever my parent sits".
                  const parent = nodes.find((n) => n.id === col.parentId);
                  if (!parent) return;
                  const targetSiblings = nodes.filter(
                    (n) => n.kind === 'collection' && n.parentId === parent.parentId,
                  );
                  moveNodes([id], parent.parentId, targetSiblings.length);
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
        },
      ];
    }
    // endpoint
    const ep = nodes.find((e) => e.id === id);
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
      ...(ep
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
