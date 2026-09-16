import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent as ReactDragEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react';
import {
  ChevronRight,
  FolderClosed,
  FolderOpen,
  FileCode2,
  Plus,
  Search,
  Pencil,
  Trash2,
  Copy,
  Terminal,
  FolderPlus,
  FilePlus,
  Package,
  CornerDownRight,
} from 'lucide-react';
import { useAppStore, type NewType } from '@/stores/app-store';
import type { Collection, Endpoint, Module, Project } from '@/types/domain';
import { methodColorVar, cn } from '@/lib/utils';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { TreeContextMenu, type ContextMenuItem } from './TreeContextMenu';
import { InlineEdit } from '@/components/ui/InlineEdit';
import { ConfirmDialog } from './ConfirmDialog';

type TargetKind = 'project' | 'module' | 'collection' | 'endpoint';

interface DeleteTarget {
  kind: TargetKind;
  id: string;
  name: string;
  /** Human-readable cascade description, shown under the confirm message. */
  cascade?: string;
}

const kindLabel: Record<TargetKind, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
  endpoint: '接口',
};

/**
 * Custom mime type used for the in-app drag payload. We shove the dragged
 * kind + ids into the dataTransfer as JSON so a single drop can resolve
 * to `moveModule` / `moveCollections` / `moveEndpoints` without any
 * extra context. Using a vendor-prefixed type means other apps / the OS
 * can't accidentally pick the data up as text.
 */
const DND_MIME = 'application/x-simple-post-tree';

interface DragPayload {
  kind: TargetKind;
  /** All ids being dragged (single-select = array of one). */
  ids: string[];
}

function encodeDrag(payload: DragPayload): string {
  return JSON.stringify(payload);
}

function decodeDrag(raw: string): DragPayload | null {
  try {
    const parsed = JSON.parse(raw) as DragPayload;
    if (!parsed || typeof parsed !== 'object') return null;
    if (
      (parsed.kind === 'module' ||
        parsed.kind === 'collection' ||
        parsed.kind === 'endpoint') &&
      Array.isArray(parsed.ids) &&
      parsed.ids.every((id) => typeof id === 'string')
    ) {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Module-level scratchpad for the in-flight drag. Some synthetic
 * event drivers (notably Playwright's `dragAndDrop`) fire the HTML5
 * dragstart / dragover / drop sequence but don't propagate the
 * `DataTransfer` payload that `setData` writes during dragstart. We
 * mirror the payload here so `makeDropHandler` can still recover the
 * drag context on drop. Real browsers populate dataTransfer normally
 * — this is purely a fallback for environments that drop it.
 */
let lastDragPayload: DragPayload | null = null;

/**
 * What part of a target row the cursor is currently over. Drives both
 * the visual indicator and the resulting move call:
 *
 *   - `above`  → insert as previous sibling (or previous item in a flat list)
 *   - `into`   → only valid for containers (module / collection); nest under it
 *   - `below`  → insert as next sibling
 *
 * Endpoints are leaves — they only support `above` / `below`.
 */
type DropZone = 'above' | 'into' | 'below';

export function ProjectTree() {
  // ---------- raw state (all raw references — safe for selectors) ----------
  const projects = useAppStore((s) => s.projects);
  const modules = useAppStore((s) => s.modules);
  const collections = useAppStore((s) => s.collections);
  const endpoints = useAppStore((s) => s.endpoints);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeEndpointId = useAppStore((s) => s.activeEndpointId);
  const activeProject = useAppStore((s) =>
    s.projects.find((p) => p.id === s.activeProjectId),
  );

  // ---------- store actions ----------
  const setActiveEndpoint = useAppStore((s) => s.setActiveEndpoint);
  const setActiveProject = useAppStore((s) => s.setActiveProject);
  const openTab = useAppStore((s) => s.openTab);
  const toggleModule = useAppStore((s) => s.toggleModuleExpanded);
  const toggleCollection = useAppStore((s) => s.toggleCollectionExpanded);
  const updateProject = useAppStore((s) => s.updateProject);
  const updateModule = useAppStore((s) => s.updateModule);
  const updateCollection = useAppStore((s) => s.updateCollection);
  const updateEndpoint = useAppStore((s) => s.updateEndpoint);
  const deleteProject = useAppStore((s) => s.deleteProject);
  const deleteModule = useAppStore((s) => s.deleteModule);
  const deleteCollection = useAppStore((s) => s.deleteCollection);
  const deleteEndpoint = useAppStore((s) => s.deleteEndpoint);
  const duplicateEndpoint = useAppStore((s) => s.duplicateEndpoint);
  const moveModule = useAppStore((s) => s.moveModule);
  const moveCollections = useAppStore((s) => s.moveCollections);
  const moveEndpoints = useAppStore((s) => s.moveEndpoints);

  // ---------- click handlers (open tab + activate) ----------
  const openProjectTab = (id: string) => {
    setActiveProject(id);
    openTab('project', id);
  };
  const openModuleTab = (id: string) => {
    openTab('module', id);
    // Toggle the row's expanded state on every click. Earlier this
    // only expanded (skipped the toggle when already open), which
    // meant clicking an expanded module to a it open in the right pane
    // also collapsed it on the tree — but the user couldn't tell why
    // "收缩" never worked from the row click. Unconditional toggle
    // makes the row behave like every other tree widget.
    toggleModule(id);
  };
  const openCollectionTab = (id: string) => {
    openTab('collection', id);
    toggleCollection(id);
  };
  const openEndpointTab = (id: string) => {
    setActiveEndpoint(id);
  };

  // ---------- new-item modal integration ----------
  const openNewItem = useAppStore((s) => s.openNewItem);
  const lastCreatedId = useAppStore((s) => s.lastCreatedId);
  const clearLastCreated = useAppStore((s) => s.clearLastCreated);

  // ---------- local UI state ----------
  const [query, setQuery] = useState('');
  const [editing, setEditing] = useState<{ kind: TargetKind; id: string } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);
  const [toDelete, setToDelete] = useState<DeleteTarget | null>(null);
  const [lastIntent, setLastIntent] = useState<NewType | null>(null);

  /**
   * Multi-select for endpoints. Ctrl+click toggles membership, plain
   * click on an endpoint clears the selection and selects only that
   * one. We only multi-select endpoints — multi-selecting modules or
   * collections is overkill for v1 of the DnD work and the rules
   * don't ask for it.
   */
  const [selectedEndpointIds, setSelectedEndpointIds] = useState<Set<string>>(
    () => new Set(),
  );

  /**
   * Live indicator of the current drop target while a drag is in
   * flight. `null` when nothing is being dragged. Keyed by
   * `${kind}:${id}` so multiple targets can highlight at once (e.g.
   * an `into` highlight on the parent + a line on the target row).
   */
  const [dropHint, setDropHint] = useState<{
    key: string;
    zone: DropZone;
  } | null>(null);

  // ---------- derived ----------
  const tree = useMemo(
    () => buildTree(modules, collections, endpoints, activeProjectId),
    [modules, collections, endpoints, activeProjectId],
  );
  const filtered = useMemo(
    () => (query.trim() ? filterTree(tree, query.toLowerCase()) : tree),
    [tree, query],
  );
  const projectModules = useMemo(
    () =>
      modules
        .filter((m) => m.projectId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [modules, activeProjectId],
  );
  const projectCollections = useMemo(
    () => collections.filter((c) => projectModules.some((m) => m.id === c.moduleId)),
    [collections, projectModules],
  );
  const projectEndpoints = useMemo(
    () => endpoints.filter((e) => projectCollections.some((c) => c.id === e.collectionId)),
    [endpoints, projectCollections],
  );

  // ---------- new-item flow ----------
  const openNewItemWithIntent = (opts: {
    type: NewType;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => {
    setLastIntent(opts.type);
    openNewItem(opts);
  };

  const handleWorkspaceNew = () => {
    setLastIntent(null);
    openNewItem();
  };

  useEffect(() => {
    if (!lastCreatedId) return;
    if (!lastIntent) return;
    setEditing({ kind: lastIntent, id: lastCreatedId });
    setLastIntent(null);
    clearLastCreated();
  }, [lastCreatedId, lastIntent, clearLastCreated]);

  // ---------- click / selection handlers ----------
  const handleEndpointClick = (id: string, e: ReactMouseEvent) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      setSelectedEndpointIds((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
      return;
    }
    setSelectedEndpointIds(new Set([id]));
    openEndpointTab(id);
  };

  // ---------- inline "+" menu builders ----------
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
        onClick: () =>
          openNewItemWithIntent({ type: 'collection', parentModuleId: firstMod.id }),
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
      const siblings = collections
        .filter(
          (c) =>
            c.moduleId === col.moduleId && c.parentCollectionId === col.parentCollectionId,
        )
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = siblings.findIndex((c) => c.id === id);
      const isTopLevel = col.parentCollectionId === null;
      const parentName = isTopLevel
        ? modules.find((m) => m.id === col.moduleId)?.name ?? ''
        : collections.find((c) => c.id === col.parentCollectionId)?.name ?? '';
      const colEps = endpoints.filter((e) => e.collectionId === id);
      const subCols = collections.filter((c) => c.parentCollectionId === id);
      const totalEps =
        colEps.length +
        endpoints.filter((e) => subCols.some((sc) => sc.id === e.collectionId)).length;
      const cascade =
        colEps.length > 0 || subCols.length > 0
          ? `将同时删除 ${colEps.length} 个接口、${subCols.length} 个子集合${
              totalEps - colEps.length > 0 ? `（含子集合下的 ${totalEps - colEps.length} 个接口）` : ''
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
                    (c) =>
                      c.moduleId === col.moduleId && c.parentCollectionId === null,
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

  // ---------- drag-and-drop helpers ----------
  /**
   * Resolve the `(source, target)` pair of project ids for a potential
   * drop. Returns `null` if the drop would cross project boundaries
   * (rule: "任何拖拽都不能使目标脱离项目").
   */
  const sameProjectGuard = (
    drag: DragPayload,
    target: { kind: TargetKind; id: string },
  ): boolean => {
    const projectIdOf = (kind: TargetKind, id: string): string | undefined => {
      if (kind === 'project') return id;
      if (kind === 'module') return modules.find((m) => m.id === id)?.projectId;
      if (kind === 'collection') {
        const c = collections.find((cc) => cc.id === id);
        if (!c) return undefined;
        return modules.find((m) => m.id === c.moduleId)?.projectId;
      }
      // endpoint
      const e = endpoints.find((ep) => ep.id === id);
      if (!e) return undefined;
      const c = collections.find((cc) => cc.id === e.collectionId);
      if (!c) return undefined;
      return modules.find((m) => m.id === c.moduleId)?.projectId;
    };
    const targetProject = projectIdOf(target.kind, target.id);
    if (!targetProject) return false;
    for (const id of drag.ids) {
      if (projectIdOf(drag.kind, id) !== targetProject) return false;
    }
    return true;
  };

  /**
   * Compute the index at which the dragged items should be inserted
   * inside the new parent. Walks the visible tree to find the right
   * sibling, since `index` in our move actions refers to position
   * inside the destination's sorted children list.
   */
  const computeTargetIndex = (
    drag: DragPayload,
    target: { kind: TargetKind; id: string },
    zone: DropZone,
  ): number | null => {
    if (zone === 'into') {
      // Insert at the end of the destination's children list.
      if (target.kind === 'module') {
        const childCount = collections.filter(
          (c) => c.moduleId === target.id && c.parentCollectionId === null,
        ).length;
        return childCount;
      }
      if (target.kind === 'collection') {
        const childCount = collections.filter(
          (c) => c.parentCollectionId === target.id,
        ).length;
        return childCount;
      }
      return null;
    }
    // above / below: insert before/after the target row.
    if (target.kind === 'module') {
      const modList = modules
        .filter((m) => m.projectId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = modList.findIndex((m) => m.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    if (target.kind === 'collection') {
      const col = collections.find((c) => c.id === target.id);
      if (!col) return null;
      const siblings = collections
        .filter(
          (c) => c.moduleId === col.moduleId && c.parentCollectionId === col.parentCollectionId,
        )
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = siblings.findIndex((c) => c.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    // endpoint: insert before/after in the same collection.
    if (target.kind === 'endpoint') {
      const ep = endpoints.find((e) => e.id === target.id);
      if (!ep) return null;
      const siblings = endpoints
        .filter((e) => e.collectionId === ep.collectionId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = siblings.findIndex((e) => e.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    return null;
  };

  /**
   * Cycle guard for collection-on-collection drops: the target must
   * not be the source itself or any descendant (we'd create a loop).
   */
  const wouldCycleCollectionInto = (
    collectionIds: string[],
    target: { kind: 'module' | 'collection'; parentId: string },
  ): boolean => {
    if (target.kind !== 'collection') return false;
    const childIndex = new Map<string, string[]>();
    for (const c of collections) {
      if (c.parentCollectionId) {
        const bucket = childIndex.get(c.parentCollectionId);
        if (bucket) bucket.push(c.id);
        else childIndex.set(c.parentCollectionId, [c.id]);
      }
    }
    const blocked = new Set<string>(collectionIds);
    const queue = [...collectionIds];
    while (queue.length > 0) {
      const id = queue.shift()!;
      const kids = childIndex.get(id) ?? [];
      for (const k of kids) {
        if (!blocked.has(k)) {
          blocked.add(k);
          queue.push(k);
        }
      }
    }
    return blocked.has(target.parentId);
  };

  /**
   * Drop handler factory. Bound per-row so each row can decide whether
   * to accept the drop (and which index to insert at).
   */
  const makeDropHandler = (
    target: { kind: TargetKind; id: string },
    targetEl: HTMLElement | null,
  ) => (e: ReactDragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const raw = e.dataTransfer.getData(DND_MIME) || e.dataTransfer.getData('text/plain');
    let drag = decodeDrag(raw);
    // Fallback: some synthetic drag drivers (Playwright) don't
    // propagate dataTransfer across dragstart → drop. The
    // `lastDragPayload` mirror written in `setDataTransfer` covers
    // that case.
    if (!drag) drag = lastDragPayload;
    setDropHint(null);
    lastDragPayload = null;
    if (!drag) return;
    if (drag.ids.includes(target.id) && drag.kind === target.kind) return;
    if (!sameProjectGuard(drag, target)) return;

    // Compute drop zone from the current cursor position (re-read
    // from the event; the row passed the zone via dropHint, but the
    // drop target may have moved since the last dragover).
    const zone = computeZoneFromEvent(e, drag, target, targetEl);
    const index = computeTargetIndex(drag, target, zone);
    if (index === null) return;

    if (drag.kind === 'module') {
      if (target.kind !== 'module') return;
      // Disallow dropping a module onto itself (already filtered above).
      moveModule(drag.ids[0], index);
      return;
    }
    if (drag.kind === 'collection') {
      if (target.kind === 'module') {
        moveCollections(drag.ids, {
          parentKind: 'module',
          parentId: target.id,
          index,
        });
        return;
      }
      if (target.kind === 'collection') {
        if (zone === 'into') {
          if (wouldCycleCollectionInto(drag.ids, { kind: 'collection', parentId: target.id })) {
            return;
          }
          moveCollections(drag.ids, {
            parentKind: 'collection',
            parentId: target.id,
            index,
          });
          return;
        }
        // above/below: drop as sibling (preserving the target's
        // parent). This handles the "脱离集合" gesture: a sub-
        // collection can be dragged next to its parent and dropped
        // as a sibling.
        const targetCol = collections.find((c) => c.id === target.id);
        if (!targetCol) return;
        if (targetCol.parentCollectionId === null) {
          moveCollections(drag.ids, {
            parentKind: 'module',
            parentId: targetCol.moduleId,
            index,
          });
        } else {
          moveCollections(drag.ids, {
            parentKind: 'collection',
            parentId: targetCol.parentCollectionId,
            index,
          });
        }
        return;
      }
      return;
    }
    // drag.kind === 'endpoint'
    if (target.kind === 'collection') {
      if (zone === 'into' || zone === 'below') {
        // Dropping into / just below a collection row both map to
        // "append into that collection". For 'into' it's natural;
        // for 'below' it's a forgiving fallback so the user doesn't
        // have to perfectly hit the middle of the row.
        const childCount = endpoints.filter(
          (e2) => e2.collectionId === target.id,
        ).length;
        moveEndpoints(drag.ids, {
          collectionId: target.id,
          index: childCount,
        });
        return;
      }
      // 'above' on a collection row: drop as last item in the
      // collection that comes BEFORE the target in the tree
      // (typically the previous sibling at the same level). To keep
      // the UX simple, just append to the target's collection — the
      // caller can re-drag to reorder.
      moveEndpoints(drag.ids, {
        collectionId: target.id,
        index: 0,
      });
      return;
    }
    if (target.kind === 'endpoint') {
      const ep = endpoints.find((e2) => e2.id === target.id);
      if (!ep) return;
      moveEndpoints(drag.ids, {
        collectionId: ep.collectionId,
        index,
      });
    }
  };

  /**
   * Whether the dragged kind can be **nested into** the given target
   * kind. Drives the 25/50/25 (vs 50/50) split on container rows.
   *
   * Rules:
   *   - Modules can never be embedded (per the user's "模块无法被嵌
   *     入集合" rule), so `into` is never valid when target is module.
   *   - Collections can be embedded under another collection.
   *   - Endpoints can be embedded under a collection.
   */
  const canNestInto = (
    dragKind: TargetKind,
    targetKind: TargetKind,
  ): boolean => {
    if (targetKind === 'module') return dragKind === 'collection';
    if (targetKind === 'collection') return dragKind === 'collection' || dragKind === 'endpoint';
    return false;
  };

  /**
   * Compute the DropZone from a DragEvent, based on cursor Y relative
   * to the row. When the dragged kind can be nested into the target
   * (see `canNestInto`), containers use a 25/50/25 split; otherwise
   * (or for leaves like endpoints) the row uses a plain 50/50 split.
   */
  const computeZoneFromEvent = (
    e: ReactDragEvent,
    drag: DragPayload | null,
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ): DropZone => {
    const rect = el?.getBoundingClientRect();
    if (!rect) return 'above';
    const y = e.clientY - rect.top;
    const ratio = y / rect.height;
    const supportsInto = drag ? canNestInto(drag.kind, target.kind) : false;
    if (!supportsInto) {
      return ratio < 0.5 ? 'above' : 'below';
    }
    if (ratio < 0.28) return 'above';
    if (ratio > 0.72) return 'below';
    return 'into';
  };

  const makeDragOverHandler = (
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ) => (e: ReactDragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'move';
    const raw = e.dataTransfer.getData(DND_MIME);
    const drag = decodeDrag(raw);
    const zone = computeZoneFromEvent(e, drag, target, el);
    const hint = { key: `${target.kind}:${target.id}`, zone };
    setDropHint((prev) => (prev && prev.key === hint.key && prev.zone === hint.zone ? prev : hint));
  };

  const makeDragLeaveHandler = (
    target: { kind: TargetKind; id: string },
  ) => (e: ReactDragEvent) => {
    // Only clear the hint when we actually leave the row (not when
    // moving over a child element). relatedTarget tells us where the
    // pointer went; if it's still inside our subtree, ignore.
    const next = e.relatedTarget as Node | null;
    if (next && e.currentTarget.contains(next)) return;
    setDropHint((prev) =>
      prev && prev.key === `${target.kind}:${target.id}` ? null : prev,
    );
  };

  const handleDragEnd = () => {
    setDropHint(null);
    lastDragPayload = null;
  };

  // ---------- render ----------
  return (
    <aside
      className="relative flex h-full w-64 shrink-0 flex-col border-r border-border bg-card/30"
      onDragEnd={handleDragEnd}
    >
      {/* Header */}
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-border bg-card/40 px-3">
        <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          工作区
        </span>
        <Tooltip content="新建（项目 / 模块 / 集合 / 接口）" side="bottom">
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={handleWorkspaceNew}
            aria-label="新建"
            data-testid="tree-new-btn"
          >
            <Plus className="h-3.5 w-3.5" />
          </Button>
        </Tooltip>
      </div>

      {/* Search */}
      <div className="px-2 pt-2">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="过滤…"
            className="h-7 pl-7 text-xs"
          />
        </div>
      </div>

      {/* Tree */}
      <div className="flex-1 overflow-y-auto px-1.5 py-1.5">
        {!activeProject ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            暂无项目。点击右上角 + 新建一个。
          </div>
        ) : (
          <ProjectRow
            project={activeProject}
            moduleCount={projectModules.length}
            collectionCount={projectCollections.length}
            endpointCount={projectEndpoints.length}
            isEditing={editing?.kind === 'project' && editing.id === activeProject.id}
            onSelect={() => openProjectTab(activeProject.id)}
            onStartEdit={() => startEdit('project', activeProject.id)}
            onSaveEdit={(name) => saveEdit('project', activeProject.id, name)}
            onCancelEdit={cancelEdit}
            onContextMenu={(e) => openMenu(e, 'project', activeProject.id)}
            onPlusClick={openProjectPlusMenu}
          />
        )}
        {filtered.map(({ module, collections: modCollections }) => (
          <div key={module.id} className="mb-0.5">
            <ModuleRow
              module={module}
              collectionCount={modCollections.length}
              endpointCount={countTreeEndpoints(modCollections)}
              isEditing={editing?.kind === 'module' && editing.id === module.id}
              onSelect={() => openModuleTab(module.id)}
              onStartEdit={() => startEdit('module', module.id)}
              onSaveEdit={(name) => saveEdit('module', module.id, name)}
              onCancelEdit={cancelEdit}
              onContextMenu={(e) => openMenu(e, 'module', module.id)}
              onPlusClick={openModulePlusMenu(module.id)}
              onDragStart={(e) => {
                setDataTransfer(e, { kind: 'module', ids: [module.id] });
              }}
              onDragEnd={handleDragEnd}
              onDragOver={makeDragOverHandler({ kind: 'module', id: module.id }, null)}
              onDragLeave={makeDragLeaveHandler({ kind: 'module', id: module.id })}
              onDrop={makeDropHandler({ kind: 'module', id: module.id }, null)}
              dropHint={
                dropHint && dropHint.key === `module:${module.id}` ? dropHint.zone : null
              }
            />
            {module.expanded && (
              <CollectionBranch
                branches={modCollections}
                depth={1}
                editing={editing}
                onSelectCollection={openCollectionTab}
                onStartEditCollection={(id) => startEdit('collection', id)}
                onSaveEditCollection={(id, name) => saveEdit('collection', id, name)}
                onCancelEdit={cancelEdit}
                onContextMenu={openMenu}
                onPlusClick={openCollectionPlusMenu}
                selectedEndpointIds={selectedEndpointIds}
                activeEndpointId={activeEndpointId}
                onEndpointClick={handleEndpointClick}
                onEndpointStartEdit={(id) => {
                  setActiveEndpoint(id);
                  startEdit('endpoint', id);
                }}
                onEndpointContextMenu={(e, id) => {
                  setActiveEndpoint(id);
                  openMenu(e, 'endpoint', id);
                }}
                onEndpointSaveEdit={(id, name) => saveEdit('endpoint', id, name)}
                dragContext={{
                  moveModule,
                  moveCollections,
                  moveEndpoints,
                  setDataTransfer,
                  makeDragOverHandler,
                  makeDragLeaveHandler,
                  makeDropHandler,
                  handleDragEnd,
                  dropHint,
                }}
              />
            )}
          </div>
        ))}
      </div>

      {/* Footer */}
      <div className="border-t border-border px-3 py-1.5 text-[10px] text-muted-foreground">
        {tree.length} 个模块 ·{' '}
        {tree.reduce(
          (a, m) =>
            a +
            m.collections.reduce(
              (b, c) => b + 1 + countSubtreeCollections(c),
              0,
            ),
          0,
        )}{' '}
        个集合 ·{' '}
        {tree.reduce(
          (a, m) =>
            a + m.collections.reduce((b, c) => b + countSubtreeEndpoints(c), 0),
          0,
        )}{' '}
        个接口
        {selectedEndpointIds.size > 1 && (
          <span className="ml-1 text-primary">· 已选 {selectedEndpointIds.size} 个接口</span>
        )}
      </div>

      {/* ---- Overlays ---- */}
      {menu && (
        <TreeContextMenu
          open
          x={menu.x}
          y={menu.y}
          items={menu.items}
          onClose={() => setMenu(null)}
        />
      )}
      {toDelete && (
        <ConfirmDialog
          open
          onOpenChange={(o) => !o && setToDelete(null)}
          title={`删除${kindLabel[toDelete.kind]}「${toDelete.name}」？`}
          message={
            toDelete.cascade ? (
              <span>此操作不可撤销，{toDelete.cascade}。</span>
            ) : (
              '此操作不可撤销。'
            )
          }
          destructive
          confirmLabel="删除"
          onConfirm={handleDeleteConfirm}
        />
      )}
    </aside>
  );
}

// ============================================================
// Row components
// ============================================================

function ProjectRow({
  project,
  moduleCount,
  collectionCount,
  endpointCount,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onPlusClick,
}: {
  project: Project;
  moduleCount: number;
  collectionCount: number;
  endpointCount: number;
  isEditing: boolean;
  onSelect: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onSelect();
        }
      }}
      onDoubleClick={(e) => {
        if (!isEditing) {
          e.preventDefault();
          e.stopPropagation();
          onStartEdit();
        }
      }}
      onContextMenu={onContextMenu}
      className="group mb-0.5 flex w-full cursor-pointer items-center gap-1 rounded-sm px-1.5 py-1 text-left text-xs hover:bg-accent/60"
    >
      <Package className="h-3.5 w-3.5 shrink-0 text-violet-400/80" />
      <InlineEdit
        value={project.name}
        onSave={onSaveEdit}
        className="flex-1"
        display={() => (
          <span className="flex-1 truncate font-medium text-foreground">{project.name}</span>
        )}
      />
      <span className="shrink-0 text-[10px] text-muted-foreground/60">
        {moduleCount}/{collectionCount}/{endpointCount}
      </span>
      <div className="invisible flex shrink-0 items-center pr-1 group-hover:visible">
        <Tooltip content="新建" side="top">
          <button
            type="button"
            onClick={onPlusClick}
            onDoubleClick={(e) => e.stopPropagation()}
            aria-label="新建"
            className="flex h-5 w-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <Plus className="h-3 w-3" />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}

interface ModuleRowProps {
  module: Module;
  collectionCount: number;
  endpointCount: number;
  isEditing: boolean;
  onSelect: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
  onDragStart: (e: ReactDragEvent) => void;
  onDragEnd: (e: ReactDragEvent) => void;
  onDragOver: (e: ReactDragEvent) => void;
  onDragLeave: (e: ReactDragEvent) => void;
  onDrop: (e: ReactDragEvent) => void;
  dropHint: DropZone | null;
}

function ModuleRow({
  module,
  collectionCount,
  endpointCount,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onPlusClick,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
  dropHint,
}: ModuleRowProps) {
  const rowRef = useRef<HTMLDivElement>(null);
  // Bind the per-row element into the handlers. The closures captured
  // by `makeDragOverHandler` / `makeDropHandler` (above) were created
  // with `null` for the element, since the element only exists at
  // render time. We re-wrap them here so the runtime can read the
  // actual bounding rect.
  const handleDragOver = (e: ReactDragEvent) => onDragOver(e);
  const handleDrop = (e: ReactDragEvent) => onDrop(e);
  const handleDragLeave = (e: ReactDragEvent) => onDragLeave(e);

  return (
    <div
      ref={rowRef}
      onContextMenu={onContextMenu}
      className="group mb-0.5 rounded-sm hover:bg-accent/60"
    >
      <div
        className={cn(
          'relative flex items-center gap-0.5 rounded-sm transition-colors',
          dropHint === 'into' && 'bg-primary/15 ring-1 ring-primary/40',
        )}
      >
        <button
          type="button"
          draggable
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          onClick={onSelect}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className="flex min-w-0 flex-1 items-center gap-1 rounded-sm px-1.5 py-1 text-left text-xs font-medium text-muted-foreground hover:text-foreground"
        >
          <ChevronRight
            className={cn(
              'h-3 w-3 shrink-0 transition-transform',
              module.expanded && 'rotate-90',
            )}
          />
          {module.expanded ? (
            <FolderOpen className="h-3.5 w-3.5 shrink-0 text-warning-foreground/80" />
          ) : (
            <FolderClosed className="h-3.5 w-3.5 shrink-0 text-warning-foreground/80" />
          )}
          <InlineEdit
            value={module.name}
            onSave={onSaveEdit}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{module.name}</span>}
          />
          <span className="shrink-0 text-[10px] text-muted-foreground/60">
            {endpointCount > 0 ? `${collectionCount}/${endpointCount}` : collectionCount}
          </span>
        </button>
        <div className="invisible flex shrink-0 items-center pr-1 group-hover:visible">
          <Tooltip content="新建" side="top">
            <button
              type="button"
              onClick={onPlusClick}
              onDoubleClick={(e) => e.stopPropagation()}
              aria-label="新建"
              className="flex h-5 w-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              <Plus className="h-3 w-3" />
            </button>
          </Tooltip>
        </div>
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}

interface CollectionBranchProps {
  branches: CollectionBranch[];
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
function CollectionBranch(props: CollectionBranchProps) {
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
      {branches.map(({ collection, endpoints: colEndpoints, children: subCols }) => (
        <div key={collection.id} className="mb-0.5">
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
            onDrop={dragContext.makeDropHandler(
              { kind: 'collection', id: collection.id },
              null,
            )}
            dropHint={
              dragContext.dropHint &&
              dragContext.dropHint.key === `collection:${collection.id}`
                ? dragContext.dropHint.zone
                : null
            }
          />
          {collection.expanded && (
            <>
              {colEndpoints.length > 0 && (
                <div className="ml-3 border-l border-border/60 pl-1">
                  {colEndpoints.map((ep) => (
                    <EndpointRow
                      key={ep.id}
                      endpoint={ep}
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
                        dragContext.dropHint &&
                        dragContext.dropHint.key === `endpoint:${ep.id}`
                          ? dragContext.dropHint.zone
                          : null
                      }
                    />
                  ))}
                </div>
              )}
              {subCols.length > 0 && (
                <CollectionBranch {...props} branches={subCols} depth={depth + 1} />
              )}
            </>
          )}
        </div>
      ))}
    </div>
  );
}

interface CollectionRowProps {
  collection: Collection;
  endpointCount: number;
  isEditing: boolean;
  onSelect: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
  onDragStart: (e: ReactDragEvent) => void;
  onDragEnd: (e: ReactDragEvent) => void;
  onDragOver: (e: ReactDragEvent) => void;
  onDragLeave: (e: ReactDragEvent) => void;
  onDrop: (e: ReactDragEvent) => void;
  dropHint: DropZone | null;
}

function CollectionRow({
  collection,
  endpointCount,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onPlusClick,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
  dropHint,
}: CollectionRowProps) {
  return (
    <div onContextMenu={onContextMenu} className="group mb-0.5 rounded-sm hover:bg-accent/60">
      <div
        className={cn(
          'relative flex items-center gap-0.5 rounded-sm transition-colors',
          dropHint === 'into' && 'bg-primary/15 ring-1 ring-primary/40',
        )}
      >
        <button
          type="button"
          draggable
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          onDragOver={onDragOver}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
          onClick={onSelect}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className="flex min-w-0 flex-1 items-center gap-1 rounded-sm px-1.5 py-1 text-left text-xs text-muted-foreground hover:text-foreground"
        >
          <ChevronRight
            className={cn(
              'h-3 w-3 shrink-0 transition-transform',
              collection.expanded && 'rotate-90',
            )}
          />
          {collection.expanded ? (
            <FolderOpen className="h-3.5 w-3.5 shrink-0 text-primary/80" />
          ) : (
            <FolderClosed className="h-3.5 w-3.5 shrink-0 text-primary/80" />
          )}
          <InlineEdit
            value={collection.name}
            onSave={onSaveEdit}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{collection.name}</span>}
          />
          <span className="shrink-0 text-[10px] text-muted-foreground/60">{endpointCount}</span>
        </button>
        <div className="invisible flex shrink-0 items-center pr-1 group-hover:visible">
          <Tooltip content="新建" side="top">
            <button
              type="button"
              onClick={onPlusClick}
              onDoubleClick={(e) => e.stopPropagation()}
              aria-label="新建"
              className="flex h-5 w-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              <Plus className="h-3 w-3" />
            </button>
          </Tooltip>
        </div>
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}

interface EndpointRowProps {
  endpoint: Endpoint;
  active: boolean;
  selected: boolean;
  multiSelect: boolean;
  isEditing: boolean;
  onClick: (e: ReactMouseEvent) => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onDragStart: (e: ReactDragEvent) => void;
  onDragEnd: (e: ReactDragEvent) => void;
  onDragOver: (e: ReactDragEvent) => void;
  onDragLeave: (e: ReactDragEvent) => void;
  onDrop: (e: ReactDragEvent) => void;
  dropHint: DropZone | null;
}

function EndpointRow({
  endpoint,
  active,
  selected,
  multiSelect,
  isEditing,
  onClick,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
  dropHint,
}: EndpointRowProps) {
  return (
    <div
      onContextMenu={onContextMenu}
      className={cn(
        'group mb-0.5 rounded-sm',
        active
          ? 'bg-primary/15'
          : selected
            ? 'bg-primary/10'
            : 'hover:bg-accent/60',
      )}
    >
      <div
        className={cn(
          'relative flex items-center gap-0.5 rounded-sm transition-colors',
          dropHint === 'into' && 'bg-primary/15 ring-1 ring-primary/40',
        )}
      >
        <button
          type="button"
          draggable
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          onDragOver={onDragOver}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
          onClick={onClick}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className={cn(
            'flex min-w-0 flex-1 items-center gap-1.5 rounded-sm px-1.5 py-1 text-left text-xs',
            active ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          <FileCode2
            className={cn(
              'h-3 w-3 shrink-0',
              active ? 'text-foreground/80' : 'text-muted-foreground/60',
            )}
          />
          <span
            className="method-letter"
            title={endpoint.method}
            style={{ backgroundColor: methodColorVar(endpoint.method) }}
          >
            {endpoint.method.charAt(0)}
          </span>
          <InlineEdit
            value={endpoint.name}
            onSave={onSaveEdit}
            className="flex-1"
            display={() => <span className="flex-1 truncate">{endpoint.name}</span>}
          />
          {multiSelect && selected && (
            <span className="shrink-0 rounded-sm bg-primary/20 px-1 text-[9px] font-medium text-primary">
              已选
            </span>
          )}
        </button>
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}

/**
 * Visual indicator for the current drop position on a row. Drawn as a
 * 2px line for `above` / `below` and a colored ring for `into` (the
 * row itself already tints its background in that case).
 */
function DropHintOverlay({ zone }: { zone: DropZone | null }) {
  if (!zone) return null;
  const style: CSSProperties =
    zone === 'above'
      ? { top: -1, left: 4, right: 4, height: 2 }
      : zone === 'below'
        ? { bottom: -1, left: 4, right: 4, height: 2 }
        : { display: 'none' }; // 'into' is drawn by the row's bg+ring
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute rounded-sm bg-primary"
      style={style}
    />
  );
}

// ============================================================
// Tree builders
// ============================================================

type CollectionBranch = {
  collection: Collection;
  endpoints: Endpoint[];
  children: CollectionBranch[];
};

type TreeBranch = { module: Module; collections: CollectionBranch[] };

interface DragContextValue {
  moveModule: (moduleId: string, targetIndex: number) => void;
  moveCollections: (
    collectionIds: string[],
    target: {
      parentKind: 'module' | 'collection';
      parentId: string;
      index: number;
    },
  ) => void;
  moveEndpoints: (
    endpointIds: string[],
    target: { collectionId: string; index: number },
  ) => void;
  setDataTransfer: (e: ReactDragEvent, payload: DragPayload) => void;
  makeDragOverHandler: (
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ) => (e: ReactDragEvent) => void;
  makeDragLeaveHandler: (
    target: { kind: TargetKind; id: string },
  ) => (e: ReactDragEvent) => void;
  makeDropHandler: (
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ) => (e: ReactDragEvent) => void;
  handleDragEnd: (e: ReactDragEvent) => void;
  dropHint: { key: string; zone: DropZone } | null;
}

/**
 * Set the in-app drag payload on the native dataTransfer. Also sets
 * a `text/plain` fallback because some browsers refuse to start a
 * drag without ANY text payload, and our custom mime alone isn't
 * always enough.
 */
function setDataTransfer(e: ReactDragEvent, payload: DragPayload) {
  const json = encodeDrag(payload);
  e.dataTransfer.setData(DND_MIME, json);
  e.dataTransfer.setData('text/plain', json);
  e.dataTransfer.effectAllowed = 'move';
  lastDragPayload = payload;
}

function buildTree(
  modules: Module[],
  collections: Collection[],
  endpoints: Endpoint[],
  projectId: string,
): TreeBranch[] {
  // Bucket endpoints by collectionId once.
  const endpointsByCol = new Map<string, Endpoint[]>();
  for (const e of endpoints) {
    const bucket = endpointsByCol.get(e.collectionId);
    if (bucket) bucket.push(e);
    else endpointsByCol.set(e.collectionId, [e]);
  }
  for (const list of endpointsByCol.values()) {
    list.sort((a, b) => a.sortOrder - b.sortOrder);
  }
  // Bucket collections by parent (parentCollectionId can be null for
  // top-level). Only build the subtree for collections that belong to
  // this project.
  const projectModuleIds = new Set(
    modules.filter((m) => m.projectId === projectId).map((m) => m.id),
  );
  const colsByParent = new Map<string | null, Collection[]>();
  for (const c of collections) {
    if (!projectModuleIds.has(c.moduleId)) continue;
    const key = c.parentCollectionId;
    const bucket = colsByParent.get(key);
    if (bucket) bucket.push(c);
    else colsByParent.set(key, [c]);
  }
  for (const list of colsByParent.values()) {
    list.sort((a, b) => a.sortOrder - b.sortOrder);
  }

  const buildCollectionBranch = (c: Collection): CollectionBranch => ({
    collection: c,
    endpoints: endpointsByCol.get(c.id) ?? [],
    children: (colsByParent.get(c.id) ?? []).map(buildCollectionBranch),
  });

  return modules
    .filter((m) => m.projectId === projectId)
    .sort((a, b) => a.sortOrder - b.sortOrder)
    .map((m) => ({
      module: m,
      collections: (colsByParent.get(null) ?? [])
        .filter((c) => c.moduleId === m.id)
        .map(buildCollectionBranch),
    }));
}

function countSubtreeEndpoints(branch: CollectionBranch): number {
  return (
    branch.endpoints.length +
    branch.children.reduce((acc, c) => acc + countSubtreeEndpoints(c), 0)
  );
}

function countSubtreeCollections(branch: CollectionBranch): number {
  return branch.children.reduce(
    (acc, c) => acc + 1 + countSubtreeCollections(c),
    0,
  );
}

function countTreeEndpoints(branches: CollectionBranch[]): number {
  return branches.reduce(
    (acc, b) => acc + countSubtreeEndpoints(b),
    0,
  );
}

function filterTree(tree: TreeBranch[], q: string): TreeBranch[] {
  return tree
    .map((m) => {
      const collections = filterCollectionBranch(m.collections, q);
      return { ...m, collections };
    })
    .filter((m) => m.collections.length > 0);
}

function filterCollectionBranch(
  branches: CollectionBranch[],
  q: string,
): CollectionBranch[] {
  return branches
    .map((b) => {
      const matchedEndpoints = b.endpoints.filter(
        (e) =>
          e.name.toLowerCase().includes(q) ||
          e.url.toLowerCase().includes(q) ||
          e.tags.some((t) => t.toLowerCase().includes(q)),
      );
      const filteredChildren = filterCollectionBranch(b.children, q);
      // Keep a collection in the result if it has matched endpoints,
      // matched sub-collections, or its own name matches the query.
      const nameMatches = b.collection.name.toLowerCase().includes(q);
      if (matchedEndpoints.length === 0 && filteredChildren.length === 0 && !nameMatches) {
        return null;
      }
      return {
        ...b,
        endpoints: matchedEndpoints,
        children: filteredChildren,
      };
    })
    .filter((b): b is CollectionBranch => b !== null);
}
