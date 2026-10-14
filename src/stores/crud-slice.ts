/**
 * CRUD 切片 —— 所有经 Tauri 改动项目、资源树、环境与 baseUrl 定义的写操作。
 *
 * 树就是一个 `nodes` 数组加一张 `endpointDetails` 映射，所以旧三表布局需要的
 * 三族动作塌缩成三个动作：
 *
 *   addNode / updateNode / deleteNode  （一次级联删除，不是三次）
 *   moveNodes                          （一次移动，不是三次）
 *   updateEndpointDetail               （配置，保持独立）
 *
 * 删除不再需要知道自己删的是什么：`nodes.parent_id` 在数据库里级联，本地写入
 * 也用同样方式移除子树。移动不再需要 `parentKind`，因为父节点自己的 `type`
 * 就决定了节点能落到哪里。
 *
 * 所有写动作都是同一种形状：
 *   1. 用 `set()` 乐观地改本地 state，让 UI 立刻更新。
 *   2. fire-and-forget（或 await）`invokeCmd<...>(COMMANDS.*)`。
 *   3. 失败时快照并回滚本地 state，然后记日志。
 *
 * 本切片通过 `get()` 读 store 其余部分（`loaded`、`setActive*` 等）——没问题，
 * 因为所有切片共用同一个 Zustand 底层 store。任何其他切片模块都不会被直接
 * import（唯一的耦合点是 `AppState` 类型）。
 *
 * 实现按关注点拆到 `./slices/`（crud-helpers / crud-project / crud-tree /
 * crud-move / crud-environment / crud-baseurl），在这里组合而不是在
 * `app-store.ts` 里注册为独立切片。多个动作刻意在同一次 `set()` 里写别人
 * 拥有的键——`deleteProject` 写 `openTabs` / `activeTabId` / `activeProjectId` /
 * `activeEndpointId`，`deleteNode` 写 `openTabs` / `activeTabId` /
 * `activeEndpointId`，`setEnvironmentActive` 写 `activeEnvironmentId`，
 * `deleteEnvironment` 写 `activeEnvironmentId` + `endpointDetails`，
 * `deleteBaseUrlDefinition` 写 `endpointDetails`；拆成多次 `get().closeTab()`
 * 式的调用会把一次 `set()` 变成多次，也改变 IPC 失败时 `set(snapshot)` 能
 * 回滚的范围。
 *
 * 拓扑是星形：子工厂可 import `./crud-helpers`，彼此之间以及与本文件之间都不
 * 互相 import。各取标准 `(set, get)` 二元组，只返回自己的动作对象。
 */
import type { StateCreator } from 'zustand';
import type { AppState } from './app-store';
import { createBaseUrlCrud } from './slices/crud-baseurl';
import type { BaseUrlCrudActions } from './slices/crud-baseurl';
import { createEnvironmentCrud } from './slices/crud-environment';
import type { EnvironmentCrudActions } from './slices/crud-environment';
import { createMoveCrud } from './slices/crud-move';
import type { MoveCrudActions } from './slices/crud-move';
import { createProjectCrud } from './slices/crud-project';
import type { ProjectCrudActions } from './slices/crud-project';
import { createTreeCrud } from './slices/crud-tree';
import type { NewNodeInput, TreeCrudActions } from './slices/crud-tree';

/** CRUD 切片拥有的 state —— 空的；所有 state 都在 bootstrap/ui/dirty/mcp 里。 */
export interface CrudSliceState {
  // 刻意留空。CRUD 切片只贡献动作；这个类型别名的存在是为了让
  // `AppState & CrudSlice` 仍然能解析。
  _crudSliceMarker?: never;
}

/** CRUD 切片拥有的动作 —— 各子工厂动作接口的并集。 */
export interface CrudSliceActions
  extends
    ProjectCrudActions,
    TreeCrudActions,
    MoveCrudActions,
    EnvironmentCrudActions,
    BaseUrlCrudActions {}

export type CrudSlice = CrudSliceState & CrudSliceActions;

/** 再导出，使 `addNode` 的入参类型仍能从这里 import。 */
export type { NewNodeInput };

export const createCrudSlice: StateCreator<AppState, [], [], CrudSlice> = (set, get, store) => ({
  ...createProjectCrud(set, get, store),
  ...createTreeCrud(set, get, store),
  ...createMoveCrud(set, get, store),
  ...createEnvironmentCrud(set, get, store),
  ...createBaseUrlCrud(set, get, store),
});
