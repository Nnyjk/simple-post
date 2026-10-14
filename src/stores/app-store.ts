/**
 * Simple Post —— 全局 store（Zustand 桶文件）。
 *
 * ---------------------------------------------------------------------------
 * 选择器规则（硬约束——加选择器前先读这段）
 * ---------------------------------------------------------------------------
 * Zustand v5 用 `Object.is` 比较选择器的返回值。任何返回新分配值的选择器
 * （`.map`、`.filter`、`.sort`、展开等）都会让消费它的组件陷入无限重渲染。
 *
 * 因此：
 *   ✅ 允许：返回 state 原始引用或单个 `.find()` 的结果
 *            如 `s => s.projects`、`s => s.endpoints.find(e => e.id === id)`
 *   ❌ 禁止：`s => s.endpoints.filter(...)`、`s => [...s.endpoints]`、
 *            `s => s.endpoints.sort(...)` 等一切会分配新值的写法
 *
 * 所有需要派生数组的过滤 / 排序 / 分组都必须放在消费组件里用 `useMemo` 做，
 * 不能放在选择器里。下面导出的几个 `useActive*` 就是标准范式。
 * ---------------------------------------------------------------------------
 *
 * 切片
 * ---------------------------------------------------------------------------
 * store 由五个 slice 工厂组合而成，每个管一块范围明确的状态。工厂是纯函数，
 * 接受标准 `(set, get, store)` 三元组，返回 state + actions 的部分对象；下面
 * `create<AppState>((set, get, store) => ({ ...createXxxSlice(...) }))` 做的就是
 * 把它们并进同一个底层 store。
 *
 * 每个切片：
 *   - 只拥有自己声明的 state 和改动这些 state 的动作。
 *   - 只通过共享的 `AppState` 类型与 store 其余部分耦合，绝不直接 import
 *     另一个切片模块。
 *   - 跨切片读用 `get()`，写用 `set()`。
 *
 * 加新功能时挑最合适的那个切片扩展；是完全新的关注点就新建切片文件，
 * 并把它的工厂追加到下面。
 * ---------------------------------------------------------------------------
 */

import { useMemo } from 'react';
import { create } from 'zustand';
import { createBootstrapSlice } from './bootstrap-slice';
import { createCrudSlice } from './crud-slice';
import { createDirtyTabsSlice } from './dirty-tabs-slice';
import { createUiSlice } from './ui-slice';
import { createMcpSlice } from './mcp-slice';
import type { DraftEndpoint } from './store-types';
import type { EndpointDetail, ResourceNode } from '@/types/domain';

// 由各切片的交集组合出 AppState。各切片工厂同时导出自己的 `*Slice` 类型与
// `create*` 函数；这里用 `*Slice` 类型（而非 `ReturnType<typeof create*>`）
// 是为了更好的 IDE hover，也能干净地丢掉用不到的 import。
import type { BootstrapSlice } from './bootstrap-slice';
import type { CrudSlice } from './crud-slice';
import type { DirtyTabsSlice } from './dirty-tabs-slice';
import type { UiSlice } from './ui-slice';
import type { McpSlice } from './mcp-slice';

/** 组合后的 AppState —— 五个切片的交集。 */
export type AppState = BootstrapSlice & CrudSlice & DirtyTabsSlice & UiSlice & McpSlice;

/**
 * 组合后的 Zustand store。各切片工厂拿到同一个 `(set, get, store)` 三元组，
 * 因此读写的都是同一个底层 store 实例——跨切片动作就是这么实现的，
 * 模块之间没有任何耦合。
 */
export const useAppStore = create<AppState>()((set, get, store) => ({
  ...createBootstrapSlice(set, get, store),
  ...createCrudSlice(set, get, store),
  ...createDirtyTabsSlice(set, get, store),
  ...createUiSlice(set, get, store),
  ...createMcpSlice(set, get, store),
}));

// ---------- 选择器 ----------
// 重要：下面每个选择器都返回 state 原始引用或单个 `.find()` 的结果。
// 不要加会分配新数组 / 对象的 selector——在 Zustand v5 下会无限重渲染。

/**
 * 当前活动项目；`bootstrap()` 还没跑完时为 `undefined`（例如后端数据到达前的
 * 首次渲染）。调用方必须容忍 `undefined` 并渲染加载态。
 */
export const useActiveProject = () =>
  useAppStore((s) => s.projects.find((p) => p.id === s.activeProjectId));

/**
 * 当前活动的工作区标签（原始引用），没有标签打开时为 `null`。返回 `OpenTab`
 * 对象本身，好让消费方按 `kind` 选渲染器。
 */
export const useActiveTab = () =>
  useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId) ?? null);

export const useActiveEnvironment = () =>
  useAppStore((s) => s.environments.find((e) => e.id === s.activeEnvironmentId));

/**
 * 活动**已存** endpoint 背后的节点，或 `null`。
 *
 * 只回答"`nodes` 里的一行"。未保存的 draft endpoint 不是——它只存在于
 * `draftEndpoints`——所以这里对它返回 `null`，且是设计如此。任何要**渲染**
 * endpoint 而不是跟数据库较劲的调用方，应该用 [`useActiveEndpointResource`]，
 * 它用同一种形状回答两种存储形式。
 *
 * 只有一个选择器，而不是 join 出来的 `{ node, detail }` 对象。Zustand v5 用
 * `Object.is` 比较选择器结果，在这里新建对象意味着每次写 store 都返回新引用，
 * 永远重渲染所有消费方。两半都是稳定引用，需要配对的调用方在自己的
 * `useMemo` 里 join，见 `useActiveEndpointResource`。
 */
export const useActiveEndpoint = () =>
  useAppStore((s) => {
    // 活动标签是 endpoint 时优先用它——它与 `setActiveEndpoint` 保持同步，
    // 即使存在陈旧引用也反映用户实际选中的那个。
    const activeTab = s.openTabs.find((t) => t.id === s.activeTabId);
    const id = activeTab?.kind === 'endpoint' ? activeTab.id : s.activeEndpointId;
    if (!id) return null;
    const node = s.nodes.find((n) => n.id === id);
    return node && node.kind === 'endpoint' ? node : null;
  });

/** 活动 endpoint 的请求配置，或 `undefined`。 */
export const useActiveEndpointDetail = () =>
  useAppStore((s) => {
    const activeTab = s.openTabs.find((t) => t.id === s.activeTabId);
    const id = activeTab?.kind === 'endpoint' ? activeTab.id : s.activeEndpointId;
    return id ? s.endpointDetails[id] : undefined;
  });

// ---------------------------------------------------------------------------
//  endpoint 资源——已存行或内存草稿，统一一种形状
// ---------------------------------------------------------------------------

/**
 * 当前标签显示的 endpoint，不论它是否已保存。
 *
 * **为什么不能只用上面那两个选择器。** 新建 endpoint 在保存前不写任何东西：
 * 它只是内存里的 `DraftEndpoint`（见 `slices/ui-draft-endpoints.ts`）。草稿既没有
 * `nodes` 条目也没有 `endpoint_details` 行，上面两个选择器对它都回答"没有"——
 * 于是每个消费方都得自己再回答一次"这里有东西吗？存过吗？"
 *
 * 补上缺的那两半，草稿就能以**同一种形状**出现在消费方面前。这不是图方便的
 * 取巧：正是它让 HTTP 测试器只需一条代码路径，未保存的 endpoint 也能用同样的
 * 编辑器、同样的校验、同样的 cURL 预览来调试——这本来就是允许"先发再存"的
 * 全部理由。
 *
 * 刻意**不**补的是 `params` / `headers` / `body`：它们在 `endpointDrafts[draftId]`
 * 里，用同一个 id 作键，且是对本函数返回的 detail 的补丁语义。测试器已有的叠加
 * （`draft?.params ?? detail?.params ?? []`）因此原样就能取到，没人编辑过的
 * 草稿也正确地报告"没有"。
 *
 * 分配发生在 `useMemo` 里，绝不在选择器里——见本文件顶部的硬约束。
 */
export function useActiveEndpointResource():
  | { id: string; isDraft: true; node: ResourceNode; detail: EndpointDetail }
  | { id: string; isDraft: false; node: ResourceNode; detail: EndpointDetail }
  | null {
  const tabKind = useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.kind);
  const savedNode = useAppStore((s) =>
    s.activeTabId
      ? (s.nodes.find((n) => n.id === s.activeTabId && n.kind === 'endpoint') ?? null)
      : null,
  );
  const savedDetail = useAppStore((s) =>
    s.activeTabId ? s.endpointDetails[s.activeTabId] : undefined,
  );
  const draft = useAppStore((s) => (s.activeTabId ? s.draftEndpoints[s.activeTabId] : undefined));

  return useMemo(() => {
    if (tabKind === 'endpoint-draft') {
      // 文档已消失的 draft 标签（被丢弃或被下方清扫剪掉）没东西可显示。
      // `null` 是诚实的答案，调用方的空状态会接住它。
      if (!draft) return null;
      return { id: draft.id, isDraft: true, node: draftNode(draft), detail: draftDetail(draft) };
    }
    if (tabKind === 'endpoint') {
      if (!savedNode || !savedDetail) return null;
      return { id: savedNode.id, isDraft: false, node: savedNode, detail: savedDetail };
    }
    return null;
  }, [tabKind, savedNode, savedDetail, draft]);
}

/**
 * 把草稿呈现为一棵树节点行。
 *
 * 草稿不在 `nodes` 里，保存之前也永远不会在，所以 `sortOrder` 是占位值——
 * 真正写入这行时由 `addNode` 分配。`updatedAt` 照抄 `createdAt`，因为这"一行"
 * 出生以来什么都没发生过。
 */
function draftNode(d: DraftEndpoint): ResourceNode {
  return {
    id: d.id,
    rootId: d.rootId,
    parentId: d.parentId,
    kind: 'endpoint',
    name: d.name,
    description: d.description,
    sortOrder: 0,
    createdAt: d.createdAt,
    updatedAt: d.createdAt,
  };
}

/**
 * 把草稿的文档字段呈现为 `EndpointDetail`。
 *
 * `params` / `headers` / `body` 故意从空开始——见 `useActiveEndpointResource`
 * 的说明。它们并没有"丢"：请求配置编辑器会写进同一 id 下的 `endpointDrafts`，
 * 而 `saveDraftEndpoint` 把两半合并成它最终写入的那一行。
 */
function draftDetail(d: DraftEndpoint): EndpointDetail {
  return {
    nodeId: d.id,
    method: d.method,
    url: d.url,
    environmentId: d.environmentId,
    baseUrlDefinitionId: d.baseUrlDefinitionId,
    baseUrlOverride: d.baseUrlOverride,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: d.auth,
    docs: d.docs,
    tags: d.tags,
  };
}

// ---------- 面向消费方的类型再导出 ----------
// 各切片把类型声明在自己的工厂旁边；这个桶再导出消费方直接从
// `@/stores/app-store` 引入的那些，调用点（`type TabKind`、`type OpenTab`、
// `type NewType` 等）不必改去认新的切片模块。保持 import 路径稳定，
// 也让日后任何切片重构对消费方不可见。

export type { Theme, RequestSettings } from './store-types';
export type { NewDraftKind, TabKind, SettingsCategoryId } from './store-types';
export type { OpenTab, EndpointDraft, NewItemDraft, ApiResponseExample } from './store-types';
export type { DraftEndpoint } from './store-types';
export { SETTINGS_TAB_ID, DRAFT_ID_PREFIX, UNTITLED_ENDPOINT_NAME } from './store-types';
