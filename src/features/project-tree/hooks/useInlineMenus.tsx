import { CornerDownRight, FileInput, FileOutput, FilePlus, FolderPlus } from 'lucide-react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { ContextMenuItem } from '../TreeContextMenu';
import type { NewDraftKind } from '@/stores/app-store';

export interface InlineMenuBindings {
  /** 当前项目的全部节点，不论类型与深度。 */
  nodes: ResourceNode[];
  /**
   * 为仍有对话框的各类打开创建*对话框*。接口刻意缺席 —— 它们由 `createEndpointAt`
   * 创建，而非通过表单。
   */
  openNewItemWithIntent: (opts: {
    type: NewDraftKind;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  /**
   * 在 `parentId` 下创建接口并丢进就地重命名。行存在后 resolve；重命名目标归调用方所有，
   * 故它可以是刚加的那一行树节点。
   */
  createEndpointAt: (parentId: string) => Promise<void>;
  /**
   * 打开导入 / 导出弹窗，落点是 `module`。集合没有这个入口：外来文档的顶层分组一律落成
   * 该模块下的集合，落点是模块才不用问「往哪一层插」。
   */
  startTransfer: (mode: 'import' | 'export', module: { id: string; name: string }) => void;
  setMenu: (m: { x: number; y: number; items: ContextMenuItem[] } | null) => void;
}

/**
 * 挂在模块 / 集合行上的就地「+」菜单。各自在点击时才惰性构造条目（项目中第一个模块 /
 * 集合在两次渲染之间可能变），故这里不 memo 任何派生数组。
 *
 * 已经没有项目级的「+」了：项目曾是一行，那一行持有「为本项目创建第一个模块」的入口。项目
 * 离开树后，那个入口就是列表顶部的「新建模块」一行。
 */
export function useInlineMenus({
  nodes,
  openNewItemWithIntent,
  createEndpointAt,
  startTransfer,
  setMenu,
}: InlineMenuBindings) {
  const openModulePlusMenu = (moduleId: string) => (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const module = nodes.find((n) => n.id === moduleId && n.kind === 'module');
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
        // 模块*就是*父级 —— 接口可以直接挂在它下面 —— 故无需先存在某个集合，也无需对话框。
        onClick: () => {
          void createEndpointAt(moduleId);
        },
      },
      { kind: 'divider' },
      {
        kind: 'item',
        label: '导入（Postman / OpenAPI）',
        icon: <FileInput className="h-3.5 w-3.5" />,
        // 模块行可能在两次渲染之间从 `nodes` 里消失（删除后菜单还开着）；找不到就不给入口，
        // 而不是拿一个空模块名去开弹窗。
        disabled: !module,
        onClick: () => module && startTransfer('import', { id: module.id, name: module.name }),
      },
      {
        kind: 'item',
        label: '导出 OpenAPI',
        icon: <FileOutput className="h-3.5 w-3.5" />,
        disabled: !module,
        onClick: () => module && startTransfer('export', { id: module.id, name: module.name }),
      },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const openCollectionPlusMenu = (collectionId: string) => (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const col = nodes.find((c) => c.id === collectionId && c.kind === 'collection');
    if (!col) return;
    // 集合的父级可能是另一个集合，故所属模块是祖先而非 `col.parentId`。对话框只用它来说明
    // 新行会落在哪里；真正的写入目标是这个集合。
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
 * 沿 `parentId` 向上走，直到找到模块行。健康的树里不可能成环，但 `seen` 守卫能防止一棵
 * 被损坏的树在点击处理器里无限打转。
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
