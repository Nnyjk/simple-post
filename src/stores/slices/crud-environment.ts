/**
 * CRUD 子工厂 —— 环境。
 *
 * `createCrudSlice` 组合的五个子工厂之一（切片级的契约——乐观 `set()` →
 * `invokeCmd` → 快照回滚——见 `../crud-slice.ts`）。
 *
 * 变量和 baseUrl 槽位上都用 `null` 作删除哨兵，而空串是真实取值——这个区别由
 * `../../__tests__/crud-slice.test.ts` 钉住，所以下面那些 `value === null` 判断
 * 是承重的，不是风格。
 *
 * `moduleBaseUrls[defId][moduleId]` 是服务 URL 唯一存放的地方：每个
 * (module × 服务) 一个独立单元格，没有继承也没有回退。
 * `resolveEndpointBaseUrl` 只读其中恰好一个。那张扁平的 `baseUrls` 映射已废弃，
 * 刻意不再写入。
 *
 * `deleteEnvironment` 与 `setEnvironmentActive` 会在与自身键同一次原子 `set()`
 * 里写别的切片拥有的键（`activeEnvironmentId`、`endpointDetails`）；拆成多次调用
 * 会改变 IPC 失败时回滚的内容。
 */
import type { StateCreator } from 'zustand';
import type { Environment, EndpointDetail, EnvWrite } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import { invokeWithRollback } from './crud-helpers';

/** CRUD 切片拥有的 environment 动作。 */
export interface EnvironmentCrudActions {
  setEnvironmentVar: (envId: string, key: string, value: string | null) => Promise<void>;
  /**
   * 停用或启用一个变量。停用的变量**照样存、照样显示，但不参与替换**。
   */
  setEnvironmentVarDisabled: (envId: string, key: string, disabled: boolean) => Promise<void>;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => Promise<void>;
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  updateEnvironment: (envId: string, patch: Partial<Pick<Environment, 'name'>>) => Promise<void>;
  /**
   * 设定（或用 `null` 清空）一个单元格：本环境下服务 `defId` 对 module `moduleId`
   * 的 URL。每个单元格都是独立取值——清一个不能碰到它的邻居。
   */
  setEnvironmentModuleBaseUrl: (
    envId: string,
    defId: string,
    moduleId: string,
    url: string | null,
  ) => Promise<void>;
  deleteEnvironment: (envId: string) => Promise<void>;
  setEnvironmentActive: (envId: string) => Promise<void>;
  /**
   * 把脚本写下的变量并进**本地副本**。
   *
   * **不发 IPC**：落库由后端做（它是唯一那条持久化路径），这里只把后端回传的
   * 「已应用」写入并进副本，于是下一个请求立刻能解析到新变量——不必等一次重新读取。
   *
   * `value` 为 `null` 的写入表示删除该变量。
   */
  applyEnvWrites: (envId: string, writes: EnvWrite[]) => void;
}

export const createEnvironmentCrud: StateCreator<AppState, [], [], EnvironmentCrudActions> = (
  set,
  get,
) => ({
  setEnvironmentVar: async (envId, key, value) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const variables = { ...prev.variables };
    if (value === null) delete variables[key];
    else variables[key] = value;
    const optimistic: Environment = {
      ...prev,
      variables,
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_set_var',
      () => invokeCmd<Environment>(COMMANDS.environmentsSetVar, { envId, key, value }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  setEnvironmentVarDisabled: async (envId, key, disabled) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const disabledVariables = prev.disabledVariables ?? [];
    const optimistic: Environment = {
      ...prev,
      disabledVariables: disabled
        ? Array.from(new Set([...disabledVariables, key])).sort()
        : disabledVariables.filter((k) => k !== key),
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_set_var_disabled',
      () => invokeCmd<Environment>(COMMANDS.environmentsSetVarDisabled, { envId, key, disabled }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  renameEnvironmentVar: async (envId, fromKey, toKey) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    if (fromKey === toKey) return;

    // 乐观的单步交换：旧键消失与新键出现发生在同一次 state 写入里，
    // 所以任何中间渲染都不会显示"变量不见了"。值跟着键走。
    const { [fromKey]: value, ...rest } = prev.variables;
    if (value === undefined) return;
    const optimistic: Environment = {
      ...prev,
      variables: { ...rest, [toKey]: value },
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));

    try {
      // 一条命令、一个事务。拆成两次 `environments_set_var` 会在两次调用之间
      // 让变量处于已删除状态，而第二次调用失败时下面的回滚只还原了 UI，
      // 后端却已经把删除提交了。
      const updated = await invokeCmd<Environment>(COMMANDS.environmentsRenameVar, {
        envId,
        fromKey,
        toKey,
      });
      // 后端回显的是改名后权威的环境，所以另一个窗口的并发编辑不会被这份
      // 乐观猜测覆盖。
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? updated : env)),
      }));
    } catch (e) {
      console.error('[store] environments_rename_var failed', e);
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? prev : env)),
      }));
    }
  },
  addEnvironment: async (projectId, name) => {
    try {
      const env = await invokeCmd<Environment>(COMMANDS.environmentsCreate, { projectId, name });
      set((s) => ({ environments: [...s.environments, env] }));
      return env.id;
    } catch (e) {
      console.error('[store] environments_create failed', e);
      return null;
    }
  },
  updateEnvironment: async (envId, patch) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const optimistic: Environment = { ...prev, ...patch };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_update',
      () => invokeCmd<Environment>(COMMANDS.environmentsUpdate, { id: envId, patch }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  setEnvironmentModuleBaseUrl: async (envId, defId, moduleId, url) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    // 两层都要拷贝：内层映射与其他所有 module 的行共享，原地改它会把这一格
    // 写进调用方根本没提过的 module。
    const nextByModule = { ...(prev.moduleBaseUrls[defId] ?? {}) };
    if (url === null) delete nextByModule[moduleId];
    else nextByModule[moduleId] = url;
    const nextModuleBaseUrls = { ...prev.moduleBaseUrls, [defId]: nextByModule };
    // 最后一个格子没了就把整个服务行删掉，免得已清空的环境还留着一个空的
    // `defId → {}` 外壳，在任何按键比对环境对象的 UI 里都读作"这里配了服务"。
    if (Object.keys(nextByModule).length === 0) delete nextModuleBaseUrls[defId];
    const optimistic: Environment = { ...prev, moduleBaseUrls: nextModuleBaseUrls };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_set_module_baseurl',
      () =>
        invokeCmd<Environment>(COMMANDS.environmentsSetModuleBaseurl, {
          envId,
          defId,
          moduleId,
          url,
        }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  deleteEnvironment: async (envId) => {
    const snapshot = {
      environments: get().environments,
      endpointDetails: get().endpointDetails,
      activeEnvironmentId: get().activeEnvironmentId,
    };
    set((s) => {
      // 把指向被删环境的 endpoint 的 `environmentId` 清空，让它不再对它解析。
      // 没有会在飞行动态重算 baseUrl 的选择器，所以这个显式置空是让 URL 栏
      // 保持诚实的关键。
      const nextDetails: Record<string, EndpointDetail> = {};
      for (const [id, d] of Object.entries(s.endpointDetails)) {
        nextDetails[id] = d.environmentId === envId ? { ...d, environmentId: null } : d;
      }
      return {
        environments: s.environments.filter((env) => env.id !== envId),
        endpointDetails: nextDetails,
        // 活动环境正好就是刚删掉的那个时，置空（无活动环境）。
        activeEnvironmentId: s.activeEnvironmentId === envId ? '' : s.activeEnvironmentId,
      };
    });
    await invokeWithRollback(
      'environments_delete',
      () => invokeCmd<void>(COMMANDS.environmentsDelete, { id: envId }),
      () => set(snapshot),
    );
  },
  setEnvironmentActive: async (envId) => {
    const prev = get().environments;
    set((s) => {
      const target = s.environments.find((env) => env.id === envId);
      if (!target) return {};
      return {
        activeEnvironmentId: envId,
        environments: s.environments.map((env) => {
          if (env.projectId !== target.projectId) return env;
          return { ...env, isActive: env.id === envId };
        }),
      };
    });
    await invokeWithRollback(
      'environments_set_active',
      () => invokeCmd<Environment>(COMMANDS.environmentsSetActive, { envId }),
      () => set({ environments: prev }),
    );
  },
  applyEnvWrites: (envId, writes) => {
    if (writes.length === 0) return;
    set((s) => ({
      environments: s.environments.map((env) => {
        if (env.id !== envId) return env;
        const variables = { ...env.variables };
        for (const write of writes) {
          if (write.value === null) delete variables[write.key];
          else variables[write.key] = write.value;
        }
        return { ...env, variables };
      }),
    }));
  },
});
