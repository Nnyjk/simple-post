/**
 * CRUD 子工厂 —— 资源树。
 *
 * `createCrudSlice` 组合的五个子工厂之一（切片级的契约——乐观 `set()` →
 * `invokeCmd` → 快照回滚——见 `../crud-slice.ts`）。
 *
 * 树就是一个 `nodes` 数组加一张 `endpointDetails` 映射，所以旧三表布局需要的
 * 三族动作塌缩成三个动作：
 *
 *   addNode / updateNode / deleteNode  （一次级联删除，不是三次）
 *   updateEndpointDetail               （配置，保持独立）
 *
 * 删除不再需要知道自己删的是什么：`nodes.parent_id` 在数据库里级联，本地写入
 * 也用同样方式移除子树。移动（见 `./crud-move.ts`）不再需要 `parentKind`，
 * 因为父节点自己的 `type` 就决定了节点能落到哪里。
 */
import type { StateCreator } from 'zustand';
import type { ResourceNode, ResourceNodeType, EndpointDetail } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { indexByParent } from '@/lib/tree-index';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import {
  blankDetail,
  collectSubtree,
  filterTabsAfterRemoval,
  invokeWithRollback,
  tabKeysForSubtree,
} from './crud-helpers';

/** 调用方在新建节点时可以设定什么。id 由我们铸造。 */
export interface NewNodeInput {
  /** 所属项目。建 module 时必填；其余情况由父节点推导。 */
  rootId?: string;
  /** `null`（或省略）把 module 放在项目顶层。 */
  parentId?: string | null;
  kind: ResourceNodeType;
  name: string;
  description?: string;
  /** 仅 endpoint 使用，且只有 `kind` 为 `'endpoint'` 时才有意义。 */
  detail?: Partial<Omit<EndpointDetail, 'nodeId'>>;
}

/** CRUD 切片拥有的资源树动作。 */
export interface TreeCrudActions {
  /**
   * 建任意类型的树节点。endpoint 可以带上 `detail`：节点与它的配置在同一个
   * 事务里写入，所以 endpoint 永远不会在没有 url 的情况下存在。
   */
  addNode: (input: NewNodeInput) => Promise<string | null>;
  /**
   * 重命名 / 改描述 / 重排一个节点。移动是 `moveNodes`——它必须同时守住类型
   * 矩阵和子树的 `rootId`，普通字段写入做不到。
   */
  updateNode: (
    id: string,
    patch: Partial<Pick<ResourceNode, 'name' | 'description' | 'sortOrder'>>,
  ) => Promise<void>;
  /**
   * 删除一个节点及其下方的一切。三种类型共用一个动作：级联写在 schema 里，
   * 本地写入通过丢掉同一棵子树来镜像它。
   */
  deleteNode: (id: string) => Promise<void>;

  /** 打补丁：修改一个 endpoint 的请求配置。 */
  updateEndpointDetail: (
    nodeId: string,
    patch: Partial<Omit<EndpointDetail, 'nodeId'>>,
  ) => Promise<void>;

  /**
   * 复制一个 endpoint。**同步**返回新 id，因为调用方（ProjectTree）会立刻激活
   * 新标签——改成异步会破坏现有交互。实现：在这里铸造 id，先乐观地把副本写进
   * store，再用同一个 id 落盘；invoke 失败时移除那份乐观副本。
   */
  duplicateEndpoint: (id: string) => string | null;
}

export const createTreeCrud: StateCreator<AppState, [], [], TreeCrudActions> = (set, get) => ({
  addNode: async (input) => {
    // 提供给后端，好让 REST 路径据此确定 endpoint 的 `detail.nodeId`——但**不**
    // 当作权威，见下面的 `createdId`。
    const id = uid();
    try {
      const node = await invokeCmd<ResourceNode>(COMMANDS.nodesCreate, {
        id,
        rootId: input.rootId ?? null,
        parentId: input.parentId ?? null,
        kind: input.kind,
        name: input.name,
        description: input.description,
        sortOrder: 0,
        ...(input.detail ? { detail: input.detail } : {}),
      });
      // 下游一切都按**服务端**返回的 id 作键，而不是我们提供的那个。REST 路径会
      // 采纳我们提供的（`ensure_server_id` 只在缺失时才填），但 Tauri 命令自己
      // 铸一个 `uid()` 并忽略该参数，所以同样一次调用在两种传输下返回不同的
      // id。用本地 `id` 给 detail 映射作键，就会把它写在一个没有任何行持有的键
      // 下：`EndpointList` 因找不到 detail 而跳过该 endpoint，标签解析成"已删除"
      // 描述符，而 module 的计数（数的是节点而非 detail）仍显示着它。信任
      // `node.id` 让两种传输一致，不必让 store 知道自己在跟谁说话。
      const createdId = node.id;
      set((s) => ({
        nodes: [...s.nodes, node],
        // 只有 endpoint 有 detail 行；给 module 写一个空的会让
        // `Object.keys(endpointDetails)` 说谎。
        ...(input.kind === 'endpoint'
          ? {
              endpointDetails: {
                ...s.endpointDetails,
                [createdId]: blankDetail(createdId, input.detail),
              },
            }
          : {}),
      }));
      return createdId;
    } catch (e) {
      console.error(`[store] nodes_create failed (${input.kind})`, e);
      return null;
    }
  },

  updateNode: async (id, patch) => {
    const prev = get().nodes.find((n) => n.id === id);
    if (!prev) return;
    const optimistic: ResourceNode = { ...prev, ...patch };
    set((s) => ({ nodes: s.nodes.map((n) => (n.id === id ? optimistic : n)) }));
    await invokeWithRollback(
      'nodes_update',
      () => invokeCmd<ResourceNode>(COMMANDS.nodesUpdate, { id, patch }),
      () => set((s) => ({ nodes: s.nodes.map((n) => (n.id === id ? prev : n)) })),
    );
  },

  deleteNode: async (id) => {
    // 三种类型共用一次删除。`nodes.parent_id` 在数据库里级联；本地写入丢掉
    // 同一棵子树，好让树不会在下一次重载之前闪回已删节点的子节点。
    const snapshot = {
      nodes: get().nodes,
      endpointDetails: get().endpointDetails,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      const removed = collectSubtree(id, indexByParent(s.nodes));
      const droppedEndpointIds = s.nodes
        .filter((n) => n.kind === 'endpoint' && removed.has(n.id))
        .map((n) => n.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const nextDetails = { ...s.endpointDetails };
      for (const eid of droppedEndpointIds) delete nextDetails[eid];
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        tabKeysForSubtree(s.nodes, removed, 'node', id),
      );
      return {
        nodes: s.nodes.filter((n) => !removed.has(n.id)),
        endpointDetails: nextDetails,
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeEndpointId:
          s.activeEndpointId && removed.has(s.activeEndpointId) ? null : s.activeEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    await invokeWithRollback(
      'nodes_delete',
      () => invokeCmd<void>(COMMANDS.nodesDelete, { id }),
      () => set(snapshot),
    );
  },

  updateEndpointDetail: async (nodeId, patch) => {
    const prev = get().endpointDetails[nodeId];
    if (!prev) return;
    const optimistic: EndpointDetail = { ...prev, ...patch };
    set((s) => ({ endpointDetails: { ...s.endpointDetails, [nodeId]: optimistic } }));
    await invokeWithRollback(
      'endpoint_detail_update',
      () => invokeCmd<EndpointDetail>(COMMANDS.endpointDetailUpdate, { nodeId, patch }),
      () => set((s) => ({ endpointDetails: { ...s.endpointDetails, [nodeId]: prev } })),
    );
  },

  /**
   * 乐观复制 + 后台 invoke。同步返回 id，好让调用方（ProjectTree）能立刻
   * 激活新标签并马上开始重命名。
   *
   * id 在这里铸造并作为 `newId` 发出，后端采纳它（`db::nodes::duplicate`
   * 不再自己铸）。这就是全部契约：一旦服务端回答了不同的 id，标签条、
   * `activeEndpointId` 和进行中的重命名会全都指着那个谁都不拥有的乐观 id，
   * 标签渲染成"接口已删除"。`addNode` 靠信任返回的 id 解决同一个问题，但复制
   * 做不到：它的调用方在本次调用返回之前就已经对那个 id 采取行动了。
   */
  duplicateEndpoint: (id) => {
    const state = get();
    const original = state.nodes.find((n) => n.id === id);
    const originalDetail = state.endpointDetails[id];
    if (!original || !originalDetail) return null;
    const newId = uid();
    const now = Date.now();
    set((s) => {
      // 放在原节点旁边，而不是集合末尾：用户看不见的副本挨着它复制的东西很容易
      // 被漏掉。这与服务端 `db::nodes::duplicate` 的做法一致。
      const siblings = s.nodes
        .filter((n) => n.parentId === original.parentId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const at = siblings.findIndex((n) => n.id === id);
      const shifted = s.nodes.map((n) =>
        n.parentId === original.parentId && n.sortOrder > original.sortOrder
          ? { ...n, sortOrder: n.sortOrder + 1 }
          : n,
      );
      return {
        nodes: [
          ...shifted,
          {
            ...original,
            id: newId,
            name: `${original.name} (副本)`,
            sortOrder: at + 1,
            createdAt: now,
            updatedAt: now,
          },
        ],
        endpointDetails: {
          ...s.endpointDetails,
          [newId]: { ...originalDetail, nodeId: newId },
        },
      };
    });
    // `newId` 不是提示——它就是那一行的 id，服务端会采纳它。所以这里 resolve 之后
    // 没有东西需要对齐：那份乐观行**就是**被写下去的那一行。只有失败才有活要干
    // ——删掉那个在别处已不存在的副本。
    invokeCmd<ResourceNode>(COMMANDS.nodesDuplicate, { id, newId }).catch((e) => {
      console.error('[store] nodes_duplicate failed', e);
      set((s) => {
        const nextDetails = { ...s.endpointDetails };
        delete nextDetails[newId];
        return { nodes: s.nodes.filter((n) => n.id !== newId), endpointDetails: nextDetails };
      });
    });
    return newId;
  },
});
