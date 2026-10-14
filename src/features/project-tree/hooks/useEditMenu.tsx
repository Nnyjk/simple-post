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
import type { NewDraftKind } from '@/stores/store-types';
import type { ResourceNode } from '@/types/domain';
import type { DeleteTarget, EditTarget, TargetKind } from '../tree-types';
import type { ContextMenuItem } from '../TreeContextMenu';

export interface EditMenuBindings {
  setEditing: Dispatch<SetStateAction<EditTarget | null>>;
  /**
   * 为仍有对话框的各类打开创建*对话框*。接口刻意缺席 —— 它由 `createEndpointAt`
   * 直接写入，故右键菜单既不需要对话框也不需要草稿。
   */
  openNewItemWithIntent: (opts: {
    type: NewDraftKind;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  /** 在 `parentId` 下创建接口并就地重命名。 */
  createEndpointAt: (parentId: string) => Promise<void>;
  openEndpointTab: (id: string) => void;
  setMenu: (m: { x: number; y: number; items: ContextMenuItem[] } | null) => void;
}

/**
 * 就地重命名 + 右键菜单构造 + 删除确认流程。`editing` 值及其 setter 由 `ProjectTree`
 * 持有（与 `useNewItemFlow` 共用，后者需要该 setter 把新建的行直接丢进重命名）；下面读
 * 到的 store 值都是原始引用或 `.find()` 的结果。
 */
export function useEditMenu({
  setEditing,
  openNewItemWithIntent,
  createEndpointAt,
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
   * 一个节点及其下所有后代，沿 `parentId` 做 BFS。
   *
   * 只用来措辞删除确认（「将同时删除 N 个集合、M 个接口」），故这是对可见树的计数，
   * 不是可以信任客户端级联视图的理由 —— 数据库无论如何都会级联。
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
        // 模块直接接受接口，故无需先存在某个集合，也无需对话框：行写入后直接进入
        // 重命名。
        {
          kind: 'item',
          label: '新建接口',
          icon: <FilePlus className="h-3.5 w-3.5" />,
          onClick: () => {
            void createEndpointAt(id);
          },
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
      // 顶层集合的父级是它的模块；嵌套集合的父级是它上面那个集合。`parentId` 本身就
      // 能说明是哪种，无需另设一个要同步的标志。
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
          onClick: () => {
            void createEndpointAt(id);
          },
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
                  // 把这个集合弹回其父级所在模块的顶层 —— 集合被误嵌套时有用。
                  //
                  // 目的地就是父级的父级：在统一的树下，「本模块的根级」就是「我父级所在的
                  // 那一层」。
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
          // 复制为 cURL —— 用 `@/lib/http-build` 的 `preparedToCurl` 渲染发送所用的
          // 那个对象。
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
                // 空操作占位：接口必须住在某个集合里，故自然的「脱离」是把它移到另一个
                // 集合。这件事交给用户拖拽 —— 点击只是给个提示。
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
