/**
 * 脏标签切片 —— 拥有那个队列：当关闭路径会丢掉草稿时，由它来斡旋
 * "这些未保存的改动该怎么办"。
 *
 * 本切片的 state：只有 `pendingDirtyClose`（队列本身）。其余输入
 * （`openTabs`、`dirtyTabIds`、`endpointDrafts`、`activeTabId`）在 UI 切片里——
 * 队列只是它们之上的一层很薄的编排。
 *
 * 和其他切片一样，本切片只通过共享的 `AppState` 类型与 store 其余部分耦合，
 * 绝不直接 import 另一个切片模块。跨切片动作（`closeTab`、
 * `updateEndpointDetail`、`clearEndpointDraft`）通过 `get().…` 调用。
 */
import type { StateCreator } from 'zustand';
import type { AppState } from './app-store';
import { draftToPatch } from './slices/crud-helpers';

/** 脏标签切片拥有的 state。 */
export interface DirtyTabsSliceState {
  /**
   * 等待用户就未保存改动表态、之后才继续关闭的脏标签 id 批次。当某条关闭路径
   * 否则会丢掉草稿时，由 `requestClose*` 动作填充。DirtyCloseDialog 读它，
   * 一次处理一个标签。
   *
   * 对话框各按钮的语义（见 resolveDirtyClose）：
   *   - "直接关闭"——关掉当前标签，并中止队列其余部分
   *     （用户其他脏标签保持打开）。
   *   - "保存"    ——保存当前标签，然后前进到下一个。
   *   - "全部保存"——保存队列里剩下的每一个标签，然后全部关掉。
   *
   * 非 endpoint 的标签（project / module / collection）只有在行内改名在途时
   * 才会"变脏"；关掉标签会强制行内输入失焦，从而触发它自己的提交——所以对这些
   * 种类来说"保存"路径实际上就是"直接关掉"。
   */
  pendingDirtyClose: { tabIds: string[]; index: number } | null;
}

/** 脏标签切片拥有的动作。 */
export interface DirtyTabsSliceActions {
  /**
   * 带脏守卫的关闭变体。若目标标签是脏的，动作会填充 `pendingDirtyClose`，
   * 让用户在真正关闭发生之前决定（丢弃 / 保存 / 全部保存）；若什么都不脏，
   * 立即关闭。
   *
   * `requestCloseTab` 是单个标签；`requestCloseOthers` 是"关掉除活动标签外的
   * 所有标签"；`requestCloseAllTabs` 是"关掉所有标签"。单个的形式供标签上的 X
   * 按钮和右键"关闭"项使用，批量形式供"Close All" / "Close Others" 使用。
   */
  requestCloseTab: (tabId: string) => void;
  requestCloseOthers: () => void;
  requestCloseAllTabs: () => void;
  /**
   * 提交给定标签上未保存的编辑而不关闭它。对 endpoint 标签意味着通过
   * `updateEndpointDetail` 应用 URL / method 草稿并清掉草稿。对其他标签，行内
   * 编辑（若有）在标签卸载时会自行提交，所以对它们是空操作。
   */
  saveDirtyTab: (tabId: string) => void;
  /**
   * 把 `pendingDirtyClose.index` 往前推一步，走到末尾则清空批次。
   * DirtyCloseDialog 的"保存" / "直接关闭" / "全部保存"按钮使用。
   */
  resolveDirtyClose: (action: 'discard' | 'save' | 'saveAll') => void;
  /** 把给定的标签 id 过滤成当前标脏的那些。请求关闭路径共用的内部助手。 */
  partitionDirty: (targetIds: string[]) => string[];
}

export type DirtyTabsSlice = DirtyTabsSliceState & DirtyTabsSliceActions;

export const createDirtyTabsSlice: StateCreator<AppState, [], [], DirtyTabsSlice> = (set, get) => ({
  // 启动时没有待关闭批次。DirtyCloseDialog 只在它变成非 null 时才挂载。
  pendingDirtyClose: null,

  // ---------- 带脏守卫的关闭路径 ----------
  // 上面那些不带守卫的 closeTab / closeAllTabs / closeSavedTabs 仍然导出，因为：
  //   - `closeTab` 是"真的关掉"那个内部原语，对话框和 delete* 动作都调它。
  //   - `closeAllTabs` 在我们**确定**没有脏标签时很方便（例如新导入的数据把一切清空）。
  // 而 WorkspaceTabs 标签条与右键菜单用的是 `requestClose*`，走脏守卫。

  /**
   * 内部助手——把某个关闭动作打算关掉的标签 id 列表分成"直接丢弃"的一批
   * （干净标签）和"问用户"的一批（脏标签）。返回脏的那部分；干净的那些由调用方
   * 通过现成的 closeTab 原语负责关闭。
   */
  partitionDirty: (targetIds: string[]) => {
    const state = get();
    const dirtySet = state.dirtyTabIds;
    return targetIds.filter((id) => dirtySet.has(id));
  },

  requestCloseTab: (tabId) => {
    const state = get();
    if (!state.openTabs.some((t) => t.id === tabId)) return;
    if (state.dirtyTabIds.has(tabId)) {
      // 脏——只为这一个打开对话框。不脏的快速路径在这里没有意义，因为没有别的
      // 标签要关。
      set({ pendingDirtyClose: { tabIds: [tabId], index: 0 } });
      return;
    }
    get().closeTab(tabId);
  },

  requestCloseOthers: () => {
    const state = get();
    const targetIds = state.openTabs.filter((t) => t.id !== state.activeTabId).map((t) => t.id);
    if (targetIds.length === 0) return;
    const dirty = get().partitionDirty(targetIds);
    // 干净的立刻关掉；脏的排进队列交给对话框。
    for (const id of targetIds) {
      if (!dirty.includes(id)) get().closeTab(id);
    }
    if (dirty.length > 0) {
      set({ pendingDirtyClose: { tabIds: dirty, index: 0 } });
    }
  },

  requestCloseAllTabs: () => {
    const state = get();
    const targetIds = state.openTabs.map((t) => t.id);
    if (targetIds.length === 0) return;
    const dirty = get().partitionDirty(targetIds);
    for (const id of targetIds) {
      if (!dirty.includes(id)) get().closeTab(id);
    }
    if (dirty.length > 0) {
      set({ pendingDirtyClose: { tabIds: dirty, index: 0 } });
    }
  },

  saveDirtyTab: (tabId) => {
    const state = get();
    const tab = state.openTabs.find((t) => t.id === tabId);
    if (!tab) return;
    if (tab.kind === 'endpoint' && state.endpointDrafts[tabId]) {
      // `saveDirtyTab` 把草稿应用到已存的 endpoint，然后清掉草稿。逻辑与 HttpTester 的
      // `handleSave` 相同，使对话框的"保存"与按钮的保存落出同样的结果。
      const draft = state.endpointDrafts[tabId]!;
      // 每个草稿字段都住在 `endpoint_details`，所以 `updateEndpointDetail` 把草稿应用在
      // 那里而不是节点上。一次跨两张表的 `updateEndpoint` 正是这次划分要避免的：重命名一个
      // endpoint 不该顺手改写请求配置。`draftToPatch` 与 HttpTester 的 Save
      // 按钮用的是同一份映射，所以对话框与按钮不可能落出不同的子集。
      const patch = draftToPatch(draft);
      if (Object.keys(patch).length > 0) state.updateEndpointDetail(tabId, patch);
      state.clearEndpointDraft(tabId);
      return;
    }
    // project / module / collection / settings —— 脏状态来自一次在途的行内改名。
    // 关掉标签（下一步）会卸载编辑器，失焦就会触发它自己的提交，所以 `saveDirtyTab`
    // 无事可做。
  },

  resolveDirtyClose: (action) => {
    const state = get();
    const batch = state.pendingDirtyClose;
    if (!batch) return;
    const currentId = batch.tabIds[batch.index];
    if (!currentId) {
      set({ pendingDirtyClose: null });
      return;
    }
    if (action === 'saveAll') {
      // 这里提交队列里剩下的每一个标签，然后全部关掉并清空批次。用不带守卫的
      // `closeTab`，否则它会重入守卫把自己重新排进队列。
      for (let i = batch.index; i < batch.tabIds.length; i++) {
        state.saveDirtyTab(batch.tabIds[i]);
        state.closeTab(batch.tabIds[i]);
      }
      set({ pendingDirtyClose: null });
      return;
    }
    if (action === 'save') {
      state.saveDirtyTab(currentId);
      state.closeTab(currentId);
      const nextIndex = batch.index + 1;
      if (nextIndex >= batch.tabIds.length) {
        set({ pendingDirtyClose: null });
      } else {
        set({ pendingDirtyClose: { tabIds: batch.tabIds, index: nextIndex } });
      }
      return;
    }
    // action === 'discard' —— 这里不保存直接关掉当前标签，并中止队列其余部分。
    // 用户其他脏标签保持打开，免得别的内容没经过他同意就丢掉。
    state.closeTab(currentId);
    set({ pendingDirtyClose: null });
  },
});
