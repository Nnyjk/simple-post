/**
 * ui-draft-endpoints —— 只存在于渲染侧的 endpoint。
 *
 * **为什么草稿不是节点。** 新建 endpoint 会立刻写一行 `nodes` 并打开它的标签，那让「新建」
 * 变成了一次提交：误点一次的用户、或只想戳个 URL 按下 Send 就走的人，会留下一行永久的
 * `未命名接口`——而树没有任何办法把那些和他们真想留下的区分开。草稿把这件事反过来：在
 * `saveDraftEndpoint` 跑之前什么都不会触及 SQLite。代价是草稿不是树行：它不在 `nodes` 里，
 * 没有 `endpoint_details` 行，任何遍历树的东西都没东西可遍历。
 *
 * **草稿的两半。** 字段住在两个地方，这个划分不是巧合：
 *
 *   draftEndpoints[id]   整份文档字段——name / method / url / auth / baseUrlOverride /
 *                        tags / docs。按文档读是因为草稿没有已存储的行可供补丁。
 *   endpointDrafts[id]   params / headers / body——请求配置编辑器写在这里，且是补丁语义，
 *                        因为写已存 endpoint 编辑的就是**同一批**编辑器。
 *
 * 复用 `endpointDrafts` 正是 HTTP 测试器能用一条代码路径对待草稿与已存 endpoint 的原因。
 * 保存时把两半合并成一次 `detail` 写入。
 *
 * **失败不具破坏性。** `saveDraftEndpoint` 在写入失败并返回 `null` 时保留草稿（以及它的标签
 * 和待保存的编辑），因为一次 IPC 失败就丢掉用户可能已经花了几分钟的 endpoint 是最糟的结果。
 * `draftSaveError` 的存在是为了让 UI 能**显示**点什么并给出重试。
 */
import type { StateCreator } from 'zustand';
import type { EndpointDetail } from '@/types/domain';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import { DRAFT_ID_PREFIX, UNTITLED_ENDPOINT_NAME } from '../store-types';
import type { DraftEndpoint } from '../store-types';
import { draftToPatch } from './crud-helpers';

export interface UiDraftEndpointsState {
  /**
   * 所有打开着的草稿 endpoint，以带 `draft:` 前缀的 id 作键——它同时也是标签 id，
   * 故「这个标签显示的是哪个 endpoint」就是一次映射读取。
   */
  draftEndpoints: Record<string, DraftEndpoint>;
  /**
   * 上一次 `saveDraftEndpoint` 失败的原因，或 `null`。与 `bootstrapError` / `nodesError` 分开，
   * 理由和那两者彼此分开一样：保存失败不该读成启动失败或树坏了。
   */
  draftSaveError: string | null;
}

export interface UiDraftEndpointsActions {
  /**
   * 在 `parentId` 下建一个草稿 endpoint 并打开它的标签。返回草稿 id。什么都不写——既不写
   * SQLite，也不写 `nodes`。
   *
   * 刻意在这里而不是在调用方开标签：草稿存在的意义就是被调试，而每一个入口（树菜单、
   * 命令面板、`createEndpointAt`）对它的要求都一样。
   *
   * 这里**不**校验父节点（与 `createEndpointAt` 不同）：这纯粹是内存操作，未知父节点在用户
   * 按 Save 之前不花任何代价——到那时写入失败、草稿还在。
   */
  createEndpointDraft: (parentId: string) => string;
  /** 就地打草稿的补丁（改名、新 url…）。未知 id 是空操作。 */
  updateDraftEndpoint: (
    draftId: string,
    patch: Partial<Omit<DraftEndpoint, 'id' | 'parentId' | 'createdAt'>>,
  ) => void;
  /** 丢掉一个草稿及其待保存编辑的所有痕迹。绝不碰数据库——那里本来就没东西可碰。 */
  discardDraftEndpoint: (draftId: string) => void;
  /**
   * 把草稿持久化成真正的 endpoint。返回新的 endpoint id；草稿已消失或写入失败时返回 `null`
   * （两种情况草稿都保留，好让调用方显示错误并让用户重试）。
   */
  saveDraftEndpoint: (draftId: string) => Promise<string | null>;
}

export type UiDraftEndpoints = UiDraftEndpointsState & UiDraftEndpointsActions;

export const createUiDraftEndpoints: StateCreator<AppState, [], [], UiDraftEndpoints> = (
  set,
  get,
) => ({
  draftEndpoints: {},
  draftSaveError: null,

  createEndpointDraft: (parentId) => {
    const state = get();
    const parent = state.nodes.find((n) => n.id === parentId);
    const id = `${DRAFT_ID_PREFIX}${uid()}`;
    const draft: DraftEndpoint = {
      id,
      parentId,
      // 父节点还没加载进来时退回活动项目，让这种草稿也知道自己最终会落在哪。
      rootId: parent?.rootId ?? state.activeProjectId,
      name: UNTITLED_ENDPOINT_NAME,
      description: '',
      // 与 `blankDetail` 本该写的默认值相同，故一个一次都没编辑就被保存的草稿产出的是逐字节相同的 endpoint。
      method: 'GET',
      url: '',
      auth: { kind: 'none' },
      baseUrlOverride: null,
      tags: [],
      docs: '',
      createdAt: Date.now(),
    };
    set((s) => ({
      draftEndpoints: { ...s.draftEndpoints, [id]: draft },
      // 从存在那一刻起就是脏的。草稿在数据库里没有行，也无法从别处恢复，故让它保持干净会让
      // 「关闭已保存」——一个它全部的承诺就是只关掉那些没有可失之物的动作——丢掉用户再也拿不
      // 回来的工作。
      dirtyTabIds: new Set(s.dirtyTabIds).add(id),
    }));
    state.openTab('endpoint-draft', id);
    // 草稿没有树行，故树里没有可高亮的行。把「当前接口」清空，别让它停在**另一个**接口上：
    // 树的高亮与标签条的激活各说各话时，用户看到的是「新建完却停在已有的那个同名接口」。
    get().setActiveEndpoint(null);
    return id;
  },

  updateDraftEndpoint: (draftId, patch) =>
    set((s) => {
      const current = s.draftEndpoints[draftId];
      // 未知 id：`updateDraftEndpoint` 静默空操作。编辑器可能在标签关闭之后才发出补丁（debounce、
      // blur、某个异步保存完成），在这里抛错会把它本该更新的那个标签弄崩。
      if (!current) return {};
      return { draftEndpoints: { ...s.draftEndpoints, [draftId]: { ...current, ...patch } } };
    }),

  discardDraftEndpoint: (draftId) =>
    set((s) => {
      const hasDraft = draftId in s.draftEndpoints;
      const hasPatch = draftId in s.endpointDrafts;
      const isDirty = s.dirtyTabIds.has(draftId);
      if (!hasDraft && !hasPatch && !isDirty) return {};
      const draftEndpoints = { ...s.draftEndpoints };
      delete draftEndpoints[draftId];
      // 请求配置的编辑也挂在草稿 id 下，而 id 从不复用——把它们留在原地是泄漏，不是缓存。
      const endpointDrafts = { ...s.endpointDrafts };
      delete endpointDrafts[draftId];
      const dirtyTabIds = new Set(s.dirtyTabIds);
      dirtyTabIds.delete(draftId);
      return { draftEndpoints, endpointDrafts, dirtyTabIds };
    }),

  saveDraftEndpoint: async (draftId) => {
    const state = get();
    const draft = state.draftEndpoints[draftId];
    if (!draft) return null;

    // 在丢弃**之前**读待保存的请求配置编辑：它们以草稿 id 作键，而 `discardDraftEndpoint` 会把它们一起丢掉。
    const pending = state.endpointDrafts[draftId];

    // 先文档字段，再把补丁盖在上面。重叠处（url / method / auth / docs / tags）以补丁为准，
    // 因为那是文档副本变陈旧期间测试器一直在写的东西，且它还是 params / headers / body 的唯一来源。
    const detail: Partial<Omit<EndpointDetail, 'nodeId'>> = {
      method: draft.method,
      url: draft.url,
      auth: draft.auth,
      baseUrlOverride: draft.baseUrlOverride,
      tags: draft.tags,
      docs: draft.docs,
      ...(pending ? draftToPatch(pending) : {}),
    };

    set({ draftSaveError: null });
    // 刻意不带 `rootId`：`nodes_create` 从父节点推导它，而 `db::nodes::create` 会**拒绝**一个与
    // 父节点不一致的值。重放草稿出生时拍下的快照会把「我编辑期间父节点被搬走了」——在可拖放的
    // 树里这是常事——变成一次硬校验失败。
    const endpointId = await state.addNode({
      parentId: draft.parentId,
      kind: 'endpoint',
      // 用户从未起名的草稿不该以空名字落盘。
      name: draft.name.trim() || UNTITLED_ENDPOINT_NAME,
      description: draft.description,
      detail,
    });

    if (!endpointId) {
      // 一切原样保留：草稿、标签、待保存的编辑。`addNode` 已经记下了真正的错误——它返回 null 而不是重新抛出。
      set({ draftSaveError: '接口草稿保存失败，请重试' });
      return null;
    }

    // 顺序有意义。先丢弃，这样即使后面某一步抛错草稿也已经没了；再把标签换过去。最后才打开
    // endpoint 标签，意味着被 `closeTab` 退役的是草稿标签，而它对 `activeTabId` 的邻居回退会
    // 立刻被覆盖。
    //
    // `pending` 刻意**不**带过去。它已经合并进 `detail`，那就是这次写入的全部内容，故在新 id
    // 下重新种一遍会重建一个本该已经结束的状态：标签上挂一个 `•`、「关闭已保存」不肯关它。
    // 已保存 endpoint 的标签是干净的——见 `HttpTester` 自己的 `handleSave`。
    //
    // 那次发送的响应挂在草稿标签下，而保存后标签换成新 id 的接口标签：把它搬过去，否则用户刚
    // 发出来的那次响应在保存之后就没了。
    get().moveResponse(draftId, endpointId);
    get().discardDraftEndpoint(draftId);
    get().closeTab(draftId);
    // 用 setActiveEndpoint 收尾：它把「当前接口」指向新行，并打开、激活它的标签。分开写这两处必然漏掉
    // 其中一处，而漏掉的那一处正是树——树的高亮读的是 `activeEndpointId`，标签条读的是
    // `activeTabId`，两处不同步时树与标签条会指向两个不同的接口。
    get().setActiveEndpoint(endpointId);
    return endpointId;
  },
});
