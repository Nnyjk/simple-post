/**
 * CRUD 子工厂 —— 环境。
 *
 * `createCrudSlice` 组合的四个子工厂之一（切片级的契约——乐观 `set()` →
 * `invokeCmd` → 快照回滚——见 `../crud-slice.ts`）。
 *
 * 变量上用 `null` 作删除哨兵，而空串是真实取值——这个区别由
 * `../../__tests__/crud-slice.test.ts` 钉住，所以下面那些 `value === null` 判断必须
 * 写对，不能当作风格偏好。
 *
 * `moduleBaseUrls[moduleId]` 是地址唯一存放的地方：每个 (环境 × 模块) 一个独立
 * 单元格，没有继承也没有回退。格子里有两个字段：`url` 是后端 api 地址，`proxy` 是
 * 实际访问地址。`resolveEndpointBaseUrl` 只读其中恰好一个。
 *
 * **本切片只写库里的东西。** 「当前环境」是客户端的实时状态，不落库，它由
 * `bootstrap-slice` 的 `setCurrentEnvironment` 管（详见那里的说明）。
 *
 * `deleteEnvironment` 会与自身键同一次原子 `set()` 里写别人拥有的键
 * （`activeEnvironmentId`）；拆成多次调用会改变 IPC 失败时回滚的内容。
 */
import type { StateCreator } from 'zustand';
import type { Environment, EnvWrite, ModuleBaseUrl } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { pickDefaultEnvironment } from '@/lib/env-default';
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
   * 写一个 (环境 × 模块) 格子里的两个字段。
   *
   * `url` 是后端 api 地址，`proxy` 是实际访问地址。两个字段各自独立：
   * `null` = 该字段保持原值，`''` = 清空该字段。两个字段都清空时这一格整个消失。
   *
   * 每个格子都是独立取值——清一个不能碰到它的邻居。
   */
  setEnvironmentModuleBaseUrl: (
    envId: string,
    moduleId: string,
    url: string | null,
    proxy: string | null,
  ) => Promise<void>;
  deleteEnvironment: (envId: string) => Promise<void>;
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
  setEnvironmentModuleBaseUrl: async (envId, moduleId, url, proxy) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    // 格子里两个字段各自独立：`null` 保持原值，`''` 清空。空串的 proxy 落成
    // `null`（没有代理），与后端 `db::environments::set_module_base_url` 同一套。
    const cell = prev.moduleBaseUrls[moduleId];
    const nextCell: ModuleBaseUrl = {
      url: url === null ? (cell?.url ?? '') : url,
      proxy: proxy === null ? (cell?.proxy ?? null) : proxy === '' ? null : proxy,
    };
    const nextModuleBaseUrls = { ...prev.moduleBaseUrls };
    // 两个字段都空了就把这一格删掉，免得已清空的环境还留着一个空的
    // `moduleId → {}` 外壳，在任何按键比对环境对象的 UI 里都读作"这里配了地址"。
    if (nextCell.url === '' && nextCell.proxy === null) {
      delete nextModuleBaseUrls[moduleId];
    } else {
      nextModuleBaseUrls[moduleId] = nextCell;
    }
    const optimistic: Environment = { ...prev, moduleBaseUrls: nextModuleBaseUrls };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_set_module_baseurl',
      () =>
        invokeCmd<Environment>(COMMANDS.environmentsSetModuleBaseurl, {
          envId,
          moduleId,
          url,
          proxy,
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
      activeEnvironmentId: get().activeEnvironmentId,
    };
    set((s) => {
      const nextEnvs = s.environments.filter((env) => env.id !== envId);
      // 当前环境正好是刚删掉的那个时，回落到这个项目的默认环境（最早创建的那个）。
      // 项目里一个环境都不剩时置空（"未选"哨兵），顶栏触发器回落到 '—'。
      //
      // 端点不再绑定环境，故这里不必动 `endpointDetails`——删掉一个环境不改变任何
      // 接口的配置，接口读的是客户端当前的「当前环境」。
      const activeEnvironmentId =
        s.activeEnvironmentId === envId
          ? (pickDefaultEnvironment(nextEnvs, s.activeProjectId)?.id ?? '')
          : s.activeEnvironmentId;
      return { environments: nextEnvs, activeEnvironmentId };
    });
    await invokeWithRollback(
      'environments_delete',
      () => invokeCmd<void>(COMMANDS.environmentsDelete, { id: envId }),
      () => set(snapshot),
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
