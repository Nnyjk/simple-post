/**
 * ui-new-item —— "新建某物"对话框的草稿、它的提交，以及唯一一条完全不用对话框
 * 的创建路径。
 *
 * **为什么草稿里没有 endpoint。** 新建 endpoint 不再问任何问题：点击本身就创建一个
 * 命名占位的**草稿**（`未命名接口`）、在工作区打开它的标签，并直接进入重命名。
 * 给它加对话框等于问三个问题（名字 / module / collection），而其中两个
 * *你点在哪里* 已经回答了。所以草稿只装那三种真的需要表单的种类：
 * `NewDraftKind` 就是 project | module | collection，没有任何代码路径能把
 * `'endpoint'` 放进去。
 *
 * 因此 `createEndpointAt` 不是"`submitNewItem` 的 endpoint 分支"——它接收显式的
 * 父节点 id、从不读草稿，而且什么都不写：它代表的那一行是由
 * `slices/ui-draft-endpoints.ts` 拥有的前端草稿，只有用户按 Save 才触及 SQLite。
 * 两条路径仍共享同一条首要不变量——展开所有祖先，让新行在它落地的位置可见——
 * 但只有对话框路径还以 `setActiveEndpoint` 收尾，因为只有它有已落盘的行可激活。
 *
 * 三种对话框种类共用一个 `submitNewItem` 而不是三个独立动作：`lastCreatedId`
 * 必须在创建 resolve **之后**落地，因为树盯着这个 id 进入重命名。拆开各分支只会
 * 白白多一跳 promise。
 */

import type { StateCreator } from 'zustand';
import type { ResourceNode } from '@/types/domain';
import { PROJECT_COLOR_DEFAULT } from '@/lib/palette';
import type { AppState } from '../app-store';
import type { NewItemDraft, NewDraftKind } from '../store-types';

/**
 * endpoint 的初始名字。刻意**不**用"新接口"：那读起来像最终名字，而这一行存在的
 * 目的就是一出现就被重命名（树会把它放进行内重命名，工作区标题也可编辑）。
 */
export const UNTITLED_ENDPOINT_NAME = '未命名接口';

const emptyDraft = (): NewItemDraft => ({
  open: false,
  type: null,
  parentModuleId: '',
  parentCollectionId: '',
  name: '',
  description: '',
  color: PROJECT_COLOR_DEFAULT,
});

export interface UiNewItemState {
  newItem: NewItemDraft;
  /**
   * `submitNewItem` 在成功创建后写入。消费方（如 ProjectTree）通过 useEffect 盯着
   * 它驱动自己的副作用（例如对刚建好的行进入行内重命名模式），然后调
   * `clearLastCreated()` 确认收到。
   */
  lastCreatedId: string | null;
}

export interface UiNewItemActions {
  openNewItem: (opts?: {
    type?: NewDraftKind;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  closeNewItem: () => void;
  setNewItemType: (type: NewDraftKind) => void;
  setNewItemName: (n: string) => void;
  setNewItemDescription: (d: string) => void;
  setNewItemColor: (c: string) => void;
  setNewItemParentModule: (id: string) => void;
  setNewItemParentCollection: (id: string) => void;
  /**
   * 提交内存中的 `newItem` 草稿，调用相应的 store 动作。因为底层的 add* 要走
   * Tauri 守护进程，所以是异步的。创建对话框 fire-and-forget 地调它——它们不需要
   * 同步拿到 id。
   */
  submitNewItem: () => Promise<string | null>;
  /**
   * 在 `parentId`（一个 module 或 collection）下建一个 endpoint，不用对话框，
   * 并在工作区打开它。
   *
   * `parentId` 就是你点的那个节点，所以没有父节点**选择**可做：endpoint 可以直接
   * 挂在 module 下，这让 module 本身就够当一个完整的父节点。
   *
   * **契约：草稿不在 `nodes` 里。** 它构建一个前端草稿
   * （`createEndpointDraft` → `slices/ui-draft-endpoints.ts`）并返回带 `draft:`
   * 前缀的 id；在 `saveDraftEndpoint` 之前什么都不会触及 SQLite。树侧必须承担
   * 的后果：
   *   - 返回的 id **不是** `nodes` 行，所以没有行可进入行内重命名、也没有可选中项。
   *     重命名发生在草稿上（`updateDraftEndpoint`）或标签标题里。
   *   - `lastCreatedId` 写的是草稿 id，所以树的 `useNewItemFlow` 监听器看到的
   *     仍是从前的那个 id。它必须认得 `draft:` id，而不是去找一行。
   *   - 草稿在 Save 之前根本不出现在树里。
   *   父节点的存在性仍在这里校验——来自删除之前所建菜单的陈旧 id 应该解析成
   * "什么都没发生"，而不是一个永远存不下去的草稿。
   */
  createEndpointAt: (parentId: string) => Promise<string | null>;
  clearLastCreated: () => void;
}

export type UiNewItem = UiNewItemState & UiNewItemActions;

/** 用户留空时，各对话框种类的兜底名字。 */
const DEFAULT_NAME: Record<NewDraftKind, string> = {
  project: '新项目',
  module: '新模块',
  collection: '新集合',
};

export const createUiNewItem: StateCreator<AppState, [], [], UiNewItem> = (set, get) => ({
  newItem: emptyDraft(),
  lastCreatedId: null,

  openNewItem: (opts) => {
    set({
      newItem: {
        ...emptyDraft(),
        open: true,
        type: opts?.type ?? null,
        parentModuleId: opts?.parentModuleId ?? '',
        parentCollectionId: opts?.parentCollectionId ?? '',
      },
    });
  },
  closeNewItem: () => {
    set({ newItem: emptyDraft() });
  },
  setNewItemType: (type) => set((s) => ({ newItem: { ...s.newItem, type } })),
  setNewItemName: (n) => set((s) => ({ newItem: { ...s.newItem, name: n } })),
  setNewItemDescription: (d) => set((s) => ({ newItem: { ...s.newItem, description: d } })),
  setNewItemColor: (c) => set((s) => ({ newItem: { ...s.newItem, color: c } })),
  setNewItemParentModule: (id) =>
    set((s) => ({
      // module 变化时一并重置 parentCollectionId，对集合对话框是安全的：
      // 集合不可能挂在用户刚离开的某个 module 的集合下。（`createEndpointAt`
      // 从不携带 collection，所以没有对应的状态。）
      newItem: { ...s.newItem, parentModuleId: id, parentCollectionId: '' },
    })),
  setNewItemParentCollection: (id) =>
    set((s) => ({ newItem: { ...s.newItem, parentCollectionId: id } })),

  submitNewItem: async () => {
    const state = get();
    const draft = state.newItem;
    if (!draft.type) return null;

    const name = draft.name.trim() || DEFAULT_NAME[draft.type];

    if (draft.type === 'project') {
      const id = await state.addProject(name, {
        description: draft.description,
        color: draft.color,
      });
      // 进入新项目：`openProject` 既是选中它也是激活它的标签，所以树立刻收敛到
      // 这个（空的）新项目，右侧显示它的卡片——名字仍可在那里编辑。项目已经
      // 不再是树里的一行，所以这是唯一让"新建"感觉落地了的动作。
      if (id) state.openProject(id);
      state.setActiveEndpoint(null);
      set({ lastCreatedId: id });
      return id;
    }

    if (draft.type === 'module') {
      const id = await state.addNode({
        rootId: state.activeProjectId,
        parentId: null,
        kind: 'module',
        name,
        description: draft.description,
      });
      set({ lastCreatedId: id });
      return id;
    }

    // draft.type === 'collection'
    // 父节点是对话框最终定下的那个：用户从某个集合行发起时就是那个集合
    // （所以"新建子集合"会正确嵌套而不是悄悄落在它旁边），否则是对话框里
    // 选定的 module。
    const parentId = draft.parentCollectionId || draft.parentModuleId;
    if (!parentId) return null;
    const parent = state.nodes.find((n) => n.id === parentId);
    if (!parent || (parent.kind !== 'module' && parent.kind !== 'collection')) return null;
    expandAncestors(state, parentId);
    const id = await state.addNode({
      // `rootId` 在服务端由父节点推导；这里再指定一次会为同一个值制造第二个真相源。
      parentId,
      kind: 'collection',
      name,
      description: draft.description,
    });
    set({ lastCreatedId: id });
    return id;
  },

  createEndpointAt: async (parentId) => {
    if (!parentId) return null;
    const state = get();
    // 父节点必须存在且能容纳 endpoint。module 可以，collection 可以，
    // project 不行（它不是树节点）。检查而不是信任调用方，是为了让删除之前
    // 所建菜单带来的陈旧 id 解析成"什么都没发生"，而不是一个 Save 永远不可能
    // 成功的草稿。
    const parent = state.nodes.find((n) => n.id === parentId);
    if (!parent || (parent.kind !== 'module' && parent.kind !== 'collection')) return null;

    expandAncestors(state, parentId);

    // `createEndpointDraft` 也会打开并激活草稿的标签，所以这里不再有
    // `setActiveEndpoint`：该动作走的是 `openTab('endpoint', id)`，那会递给
    // 工作区一个并不存在的行的标签。
    const draftId = state.createEndpointDraft(parentId);
    // 与各对话框种类的行一样发布给树。见上面的契约说明：消费方必须把 `draft:`
    // id 当作"出现了一个草稿"，而不是"找到这一行并重命名它"。
    set({ lastCreatedId: draftId });
    return draftId;
  },

  clearLastCreated: () => set({ lastCreatedId: null }),
});

/**
 * 展开 `nodeId` 及其上方的每一级祖先，从最近的一级开始。
 *
 * 从**父节点本身**（而不是它之上）开始是刻意的：在折叠的集合里建集合必须把那个
 * 集合也一起露出来。
 */
function expandAncestors(state: AppState, nodeId: string): void {
  let cur: ResourceNode | undefined = state.nodes.find((n) => n.id === nodeId);
  while (cur) {
    if (!cur.expanded) state.toggleNodeExpanded(cur.id);
    cur = cur.parentId ? state.nodes.find((n) => n.id === cur!.parentId) : undefined;
  }
}
