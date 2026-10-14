/**
 * ui-draft-endpoints —— 只存在于渲染侧的 endpoint。
 *
 * **为什么草稿不是节点。** 新建 endpoint 会立刻写一行 `nodes` 并打开它的标签，
 * 那让"新建"变成了一次提交：误点一次的用户、或只想戳个 URL 按下 Send 就走的人，
 * 会留下一行永久的 `未命名接口`——而树没有任何办法把那些和他们真想留下的区分开。
 * 草稿把这件事反过来：在 `saveDraftEndpoint` 跑之前什么都不会触及 SQLite，所以
 * 整条创建流程（起名、输 URL、发一次、丢弃）零写入、随时可放弃。代价是草稿不是
 * 树行：它不在 `nodes` 里，没有 `endpoint_details` 行，无论重载工作区多少次也
 * 不会凭空出现。任何遍历树的东西都没东西可遍历。
 *
 * **草稿的两半。** 字段住在两个地方，这个划分不是巧合：
 *
 *   draftEndpoints[id]   整份文档字段——name / method / url / auth /
 *                        environmentId / baseUrl* / tags / docs。按文档读是因为
 *                        草稿没有已存储的行可供补丁，否则"未设置"与"默认为空"
 *                        就无从分辨。
 *   endpointDrafts[id]   params / headers / body——请求配置编辑器写在这里，且是
 *                        补丁语义，因为写已存 endpoint 编辑的就是**同一批**编辑器。
 *
 * 复用 `endpointDrafts` 正是 HTTP 测试器能用一条代码路径对待草稿与已存 endpoint
 * 的原因：它画的叠加层完全一样，未保存的 endpoint 也能用与已存的一样 UI 调试。
 * 保存时把两半合并成一次 `detail` 写入。
 *
 * **失败不具破坏性。** `saveDraftEndpoint` 在写入失败并返回 `null` 时保留草稿
 * （以及它的标签和待保存的编辑），因为另一个选项——因为一次 IPC 失败就丢掉用户
 * 可能已经花了几分钟的 endpoint——是最糟的结果。`addNode` 已经记下底层错误并
 * 把它吞成 `null`，所以这里的消息必然是通用的；`draftSaveError` 的存在是为了让
 * UI 能**显示**点什么并给出重试，而不是看起来什么都没发生。
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
   * 所以"这个标签显示的是哪个 endpoint"就是一次映射读取。
   */
  draftEndpoints: Record<string, DraftEndpoint>;
  /**
   * 上一次 `saveDraftEndpoint` 失败的原因，或 `null`。与 `bootstrapError` /
   * `nodesError` 分开，理由和那两者彼此分开一样：保存失败不该读成启动失败或
   * 树坏了。
   */
  draftSaveError: string | null;
}

export interface UiDraftEndpointsActions {
  /**
   * 在 `parentId` 下建一个草稿 endpoint 并打开它的标签。返回草稿 id。什么都不写
   * ——既不写 SQLite，也不写 `nodes`。
   *
   * 刻意在这里而不是在调用方开标签：草稿存在的意义就是被调试，而每一个入口
   * （树菜单、命令面板、`createEndpointAt`）对它的要求都一样。另一种做法是三个
   * 调用点各自记得开一个，那正是草稿在标签条里看不见的原因。
   *
   * 这里**不**校验父节点（与 `createEndpointAt` 不同）：这纯粹是内存操作，未知父
   * 节点在用户按 Save 之前不花任何代价——到那时写入失败、草稿还在。在知道用户
   * 意图的那个入口校验一次就够了。
   */
  createEndpointDraft: (parentId: string) => string;
  /** 就地打草稿的补丁（改名、新 url…）。未知 id 是空操作。 */
  updateDraftEndpoint: (
    draftId: string,
    patch: Partial<Omit<DraftEndpoint, 'id' | 'parentId' | 'createdAt'>>,
  ) => void;
  /**
   * 丢掉一个草稿及其待保存编辑的所有痕迹。绝不碰数据库——那里本来就没东西可碰。
   */
  discardDraftEndpoint: (draftId: string) => void;
  /**
   * 把草稿持久化成真正的 endpoint。返回新的 endpoint id；草稿已消失或写入失败时
   * 返回 `null`（两种情况草稿都保留，好让调用方显示错误并让用户重试）。
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
      // 与 `blankDetail` 本该写的默认值相同，所以一个一次都没编辑就被保存的
      // 草稿产出的是逐字节相同的 endpoint。
      method: 'GET',
      url: '',
      auth: { kind: 'none' },
      environmentId: null,
      baseUrlDefinitionId: null,
      baseUrlOverride: null,
      tags: [],
      docs: '',
      createdAt: Date.now(),
    };
    set((s) => ({
      draftEndpoints: { ...s.draftEndpoints, [id]: draft },
      // 从存在那一刻起就是脏的。草稿在数据库里没有行，也无法从别处恢复，
      // 所以让它保持干净会让"关闭已保存"——一个它全部的承诺就是只关掉那些
      // 没有可失之物的动作——丢掉用户再也拿不回来的工作。这个点同时也是诚实的
      // 描述：这个标签还没有任何东西被保存。
      dirtyTabIds: new Set(s.dirtyTabIds).add(id),
    }));
    state.openTab('endpoint-draft', id);
    return id;
  },

  updateDraftEndpoint: (draftId, patch) =>
    set((s) => {
      const current = s.draftEndpoints[draftId];
      // 未知 id：静默空操作。编辑器可能在标签关闭之后才发出补丁（debounce、blur、
      // 某个异步保存落地），在这里抛错会把它本该更新的那个标签弄崩。
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
      // 请求配置的编辑也挂在草稿 id 下，而 id 从不复用——把它们留在原地是泄漏，
      // 不是缓存。
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

    // 在丢弃**之前**读待保存的请求配置编辑：它们以草稿 id 作键，
    // 而 `discardDraftEndpoint` 会把它们一起丢掉。
    const pending = state.endpointDrafts[draftId];

    // 先文档字段，再把补丁盖在上面。重叠处（url / method / auth / docs / tags）
    // 以补丁为准，因为那是文档副本变陈旧期间测试器一直在写的东西，而且它还是
    // params / headers / body 的唯一来源。
    const detail: Partial<Omit<EndpointDetail, 'nodeId'>> = {
      method: draft.method,
      url: draft.url,
      auth: draft.auth,
      environmentId: draft.environmentId,
      baseUrlDefinitionId: draft.baseUrlDefinitionId,
      baseUrlOverride: draft.baseUrlOverride,
      tags: draft.tags,
      docs: draft.docs,
      ...(pending ? draftToPatch(pending) : {}),
    };

    set({ draftSaveError: null });
    // 刻意不带 `rootId`：`nodes_create` 从父节点推导它，而 `db::nodes::create`
    // 会**拒绝**一个与父节点不一致的值。重放草稿出生时拍下的快照会把
    // "我编辑期间父节点被搬走了"——在可拖放的树里这是常事——变成一次硬校验失败。
    // 推导出来的答案天然是正确的。
    const endpointId = await state.addNode({
      parentId: draft.parentId,
      kind: 'endpoint',
      // 用户从未起名的草稿不该以空名字落盘。
      name: draft.name.trim() || UNTITLED_ENDPOINT_NAME,
      description: draft.description,
      detail,
    });

    if (!endpointId) {
      // 一切原样保留：草稿、标签、待保存的编辑。`addNode` 已经记下了真正的错误
      // ——它返回 null 而不是重新抛出。
      set({ draftSaveError: '接口草稿保存失败，请重试' });
      return null;
    }

    // 顺序有意义。先丢弃，这样即使后面某一步抛错草稿也已经没了；再把标签换过去。
    // 最后才打开 endpoint 标签，意味着被 `closeTab` 退役的是草稿标签，而它对
    // `activeTabId` 的邻居回退会立刻被覆盖。
    //
    // `pending` 刻意**不**带过去。它上面已经合并进 `detail`，那就是这次写入的
    // 全部内容，所以在新 id 下重新种一遍恰恰会重建 Save 本该终结的那个状态：
    // 标签上挂一个 `•`、"关闭已保存"不肯关它、第二次 Save 又写不出新东西。
    // 已保存 endpoint 的标签是干净的——见 `HttpTester` 自己的 `handleSave`，
    // 出于同样的理由它调了 `clearEndpointDraft`。
    get().discardDraftEndpoint(draftId);
    get().closeTab(draftId);
    get().openTab('endpoint', endpointId);
    get().setActiveTab(endpointId);
    return endpointId;
  },
});
