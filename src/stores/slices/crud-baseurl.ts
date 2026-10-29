/**
 * CRUD 子工厂 —— 项目级的 baseUrl 服务定义。
 *
 * `createCrudSlice` 组合的五个子工厂之一（切片级的契约——乐观 `set()` →
 * `invokeCmd` → 快照回滚——见 `../crud-slice.ts`）。
 *
 * 服务属于**项目**，不属于 module——每个 module 看到同一份列表，所以下面这些
 * 动作全都以 `projectId` 为键。模型里的按 module 部分是 URL，住在
 * `environment.moduleBaseUrls[defId][moduleId]`，由 `crud-environment.ts` 写入。
 *
 * 删除定义仍要把指向它的 endpoint 置空；环境里为它填过的地址由服务端一并清掉
 * （`db::base_url_defs::delete` 在同一个事务里删掉那些格子），前端这边不必再管。
 */
import type { StateCreator } from 'zustand';
import type { BaseUrlDefinition, EndpointDetail, Project } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import { invokeWithRollback } from './crud-helpers';

/** CRUD 切片拥有的 baseUrl 定义动作。 */
export interface BaseUrlCrudActions {
  /**
   * 往 `projectId` 加一个服务。同步返回它的 id，好让调用方无需往返就能为它
   * 填一个环境单元格。
   */
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (
    projectId: string,
    defId: string,
    patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>,
  ) => Promise<void>;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => Promise<void>;
  /**
   * 把某个服务设为**项目**默认，或用 `null` 清掉 ★。★ 是项目级的——没有按 module
   * 的默认值——所以没设 `baseUrlDefinitionId` 的 endpoint 就通过它解析。清掉 ★
   * 是一种真实的（虽然是有意的）做法：让那些 endpoint 没有 baseUrl。
   */
  setDefaultBaseUrlDefinition: (projectId: string, defId: string | null) => Promise<void>;
}

export const createBaseUrlCrud: StateCreator<AppState, [], [], BaseUrlCrudActions> = (
  set,
  get,
) => ({
  /**
   * 乐观本地新增 + 后台 invoke。同步返回 id，因为设置表会把它直接传给
   * `setEnvironmentModuleBaseUrl` 去填一个环境单元格。
   *
   * id 在**这里**铸造、后端采纳（`opts.id`），这正是重点。本切片里有两处在
   * 这一行落盘之前就按服务 id 作键——项目的 ★ 和每个
   * `env.moduleBaseUrls[defId][moduleId]` 单元格——后端铸的 id 会把两者都变成
   * 指向不存在行的键：单元格写入已经按旧键进了数据库，事后再换掉这行也搬不动
   * 它。`resolveEndpointBaseUrl` 于是读到一个不存在的单元格并返回 `''`。
   *
   * 项目的第一个服务在这里成为默认。后端也这么做，但本地镜像一下能挡住 ★ 在
   * 唯一要紧的地方迟到一瞬——没选服务的 endpoint 就是通过它解析的。
   */
  addBaseUrlDefinition: (projectId, name) => {
    const defId = uid();
    const label = name.trim() || '未命名';
    set((s) => ({
      projects: s.projects.map((p) => {
        if (p.id !== projectId) return p;
        const wasEmpty = p.baseUrlDefinitions.length === 0;
        return {
          ...p,
          baseUrlDefinitions: [...p.baseUrlDefinitions, { id: defId, name: label }],
          // 方便起见：没有 ★ 时选择器就没有任何东西能作为项目默认。
          defaultBaseUrlDefinitionId: wasEmpty ? defId : p.defaultBaseUrlDefinitionId,
        };
      }),
    }));
    invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsCreate, {
      projectId,
      name: label,
      opts: { id: defId },
    })
      .then((persisted) => {
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.map((d) => (d.id === defId ? persisted : d)),
            };
          }),
        }));
      })
      .catch((e) => {
        console.error('[store] base_url_defs_create failed', e);
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.filter((d) => d.id !== defId),
              // 这一行没了，所以指着它的 ★ 是悬空指针：`resolveEndpointBaseUrl` 会去查它、
              // 查不到，然后给项目里每个 endpoint 一个空 base URL——正是这次创建
              // 已经造成过一次的故障。把 ★ 收回空，而不是去顶替一个用户从未
              // 选过的服务。
              defaultBaseUrlDefinitionId:
                p.defaultBaseUrlDefinitionId === defId ? null : p.defaultBaseUrlDefinitionId,
            };
          }),
        }));
      });
    return defId;
  },
  updateBaseUrlDefinition: async (projectId, defId, patch) => {
    const prevProject = get().projects.find((p) => p.id === projectId);
    const prevDef = prevProject?.baseUrlDefinitions.find((d) => d.id === defId);
    if (!prevProject || !prevDef) return;
    const optimistic: Project = {
      ...prevProject,
      baseUrlDefinitions: prevProject.baseUrlDefinitions.map((d) =>
        d.id === defId ? { ...d, ...patch } : d,
      ),
    };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? optimistic : p)),
    }));
    await invokeWithRollback(
      'base_url_defs_update',
      () => invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsUpdate, { id: defId, patch }),
      () =>
        set((s) => ({
          projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
        })),
    );
  },
  deleteBaseUrlDefinition: async (projectId, defId) => {
    const snapshot = {
      projects: get().projects,
      endpointDetails: get().endpointDetails,
    };
    set((s) => {
      // 级联：任何指向这个 def 的 endpoint 都退回项目默认。各环境里为它填过的
      // 地址由服务端在同一个事务里删掉（`db::base_url_defs::delete`），所以这里
      // 这里不用管 `moduleBaseUrls`——那些格子连同孤儿键一起消失。
      const nextDetails: Record<string, EndpointDetail> = {};
      for (const [id, d] of Object.entries(s.endpointDetails)) {
        nextDetails[id] = d.baseUrlDefinitionId === defId ? { ...d, baseUrlDefinitionId: null } : d;
      }
      return {
        projects: s.projects.map((p) => {
          if (p.id !== projectId) return p;
          return {
            ...p,
            baseUrlDefinitions: p.baseUrlDefinitions.filter((d) => d.id !== defId),
            // ★ 指着的一行已经不存在了；留着会让设置表高亮不到任何东西，
            // 也会让解析器去查一个永远查不到的 def。
            defaultBaseUrlDefinitionId:
              p.defaultBaseUrlDefinitionId === defId ? null : p.defaultBaseUrlDefinitionId,
          };
        }),
        endpointDetails: nextDetails,
      };
    });
    await invokeWithRollback(
      'base_url_defs_delete',
      () => invokeCmd<void>(COMMANDS.baseUrlDefsDelete, { id: defId }),
      () => set(snapshot),
    );
  },
  setDefaultBaseUrlDefinition: async (projectId, defId) => {
    const prevProject = get().projects.find((p) => p.id === projectId);
    if (!prevProject) return;
    // 一个字段、一次 `set()`：★ 是单个 id 而不是散在各行上的标记，
    // 所以不存在"两个服务同时是默认"的中间态。
    const optimistic: Project = { ...prevProject, defaultBaseUrlDefinitionId: defId };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? optimistic : p)),
    }));
    await invokeWithRollback(
      'base_url_defs_set_default',
      () => invokeCmd<Project>(COMMANDS.baseUrlDefsSetDefault, { projectId, defId }),
      () =>
        set((s) => ({
          projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
        })),
    );
  },
});
