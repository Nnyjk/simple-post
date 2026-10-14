/**
 * CRUD 子工厂 —— 项目。
 *
 * `createCrudSlice` 组合的五个子工厂之一（切片级的契约——乐观 `set()` →
 * `invokeCmd` → 快照回滚——见 `../crud-slice.ts`）。
 *
 * `deleteProject` 是本切片里跨度最大的跨切片写入：它在一次原子的 `set()` 里
 * 移除该项目的整棵子树、它的环境、按被丢弃 endpoint 作键的响应历史 / 已存示例，
 * 以及对应的工作区标签。保持单次写入是刻意的：改写成多次
 * `get().closeTab()` 式的调用会把它拆成好几次渲染，也让 IPC 失败时的
 * `set(snapshot)` 回滚到一个用户已经走过的状态。
 */
import type { StateCreator } from 'zustand';
import type { Project, Environment, ResourceNode } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import { filterTabsAfterRemoval, invokeWithRollback, tabKeysForSubtree } from './crud-helpers';

/** CRUD 切片拥有的 project 动作。 */
export interface ProjectCrudActions {
  addProject: (
    name: string,
    opts?: { description?: string; color?: string },
  ) => Promise<string | null>;
  updateProject: (id: string, patch: Partial<Omit<Project, 'id' | 'createdAt'>>) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
}

export const createProjectCrud: StateCreator<AppState, [], [], ProjectCrudActions> = (
  set,
  get,
) => ({
  addProject: async (name, opts) => {
    try {
      const project = await invokeCmd<Project>(COMMANDS.projectsCreate, {
        name,
        opts: opts ?? null,
      });
      set((s) => ({ projects: [...s.projects, project] }));
      // 新项目会带一个默认环境**和**一个默认 module（见
      // `db::projects::create_with_defaults`）。两者都不重新拉取：bootstrap 期间
      // 第二层已经跑过，`openProject` 的 `loadWorkspace` 会提前返回，所以不补这
      // 两步的话，环境切换器会显示"—"而 `{{var}}` 无从解析，树会对一个明明有
      // module 的项目显示「该项目还没有模块」——而且一直持续到应用重启。
      //
      // 两者都是不带过滤的整表读取（所有项目的环境、所有项目的节点），也正是
      // store 已经持有的内容——所以是替换而不是合并。module 没有
      // `endpoint_details` 行，这里 detail 映射无需处理。
      try {
        const environments = await invokeCmd<Environment[]>(COMMANDS.environmentsList);
        set({ environments });
      } catch (e) {
        // 项目本身已经存在；环境刷新失败不等于创建失败，让整个动作失败会在磁盘上
        // 留下一个 store 不知道的项目。
        console.error('[store] environments_list after create failed', e);
      }
      try {
        const nodes = await invokeCmd<ResourceNode[]>(COMMANDS.nodesList);
        set({ nodes, nodesLoaded: true });
      } catch (e) {
        // 同理：种子 module 已在磁盘上，只是 store 暂时显示不出来。`nodesLoaded`
        // 也一样——它绝不能停在 `false`，否则树会永远渲染加载态，
        // 而 `pruneStaleTabs` 会一直空转。
        console.error('[store] nodes_list after create failed', e);
      }
      return project.id;
    } catch (e) {
      console.error('[store] projects_create failed', e);
      return null;
    }
  },
  updateProject: async (id, patch) => {
    // 乐观本地更新 + 后台 invoke；失败回滚。
    const prev = get().projects.find((p) => p.id === id);
    if (!prev) return;
    const optimistic: Project = { ...prev, ...patch, updatedAt: Date.now() };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? optimistic : p)),
    }));
    await invokeWithRollback(
      'projects_update',
      () => invokeCmd<Project>(COMMANDS.projectsUpdate, { id, patch }),
      () =>
        set((s) => ({
          projects: s.projects.map((p) => (p.id === id ? prev : p)),
        })),
    );
  },
  deleteProject: async (id) => {
    // 回滚用的快照。
    const snapshot = {
      projects: get().projects,
      nodes: get().nodes,
      endpointDetails: get().endpointDetails,
      environments: get().environments,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeProjectId: get().activeProjectId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      // `rootId` 被反规范化到每个节点身上，正是为了让"什么属于这个项目"变成一次
      // 过滤，而不必沿 module → collection → endpoint 一路向下走。
      const removed = new Set(s.nodes.filter((n) => n.rootId === id).map((n) => n.id));
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
        tabKeysForSubtree(s.nodes, removed, 'project', id),
      );
      const nextActiveEndpointId =
        s.activeEndpointId && removed.has(s.activeEndpointId) ? null : s.activeEndpointId;
      return {
        projects: s.projects.filter((p) => p.id !== id),
        nodes: s.nodes.filter((n) => !removed.has(n.id)),
        endpointDetails: nextDetails,
        environments: s.environments.filter((env) => env.projectId !== id),
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        // 删掉在前面的项目时，把用户送到幸存的那个；它是最后一个时回欢迎页。
        //
        // 空串是切表面的开关（`App.tsx` 在 `activeProjectId === ''` 时渲染欢迎
        // 选择器，而欢迎页有"新建项目"卡片兜住空状态）。把 `activeProjectId`
        // 留在刚删掉的 id 上会让 `App.tsx` 待在工作区分支里对着一个不存在的
        // 项目：树显示"尚未选择项目"，右侧显示"该项目已不存在"，整个画面没有一处
        // 说明该怎么办。
        activeProjectId:
          s.activeProjectId === id
            ? (s.projects.find((p) => p.id !== id)?.id ?? '')
            : s.activeProjectId,
        activeEndpointId: nextActiveEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    await invokeWithRollback(
      'projects_delete',
      () => invokeCmd<void>(COMMANDS.projectsDelete, { id }),
      () => set(snapshot),
    );
  },
});
