/**
 * CRUD 子工厂 —— 环境。
 *
 * `createCrudSlice` 组合的四个子工厂之一（切片级的契约——乐观 `set()` → `invokeCmd` →
 * 快照回滚——见 `../crud-slice.ts`）。
 *
 * 变量上用 `null` 作删除哨兵，而空串是真实取值——这个区别由
 * `../../__tests__/crud-slice.test.ts` 钉住，故那些 `value === null` 判断必须写对。
 *
 * `moduleBaseUrls[moduleId]` 是地址唯一存放的地方：每个 (环境 × 模块) 一个独立单元格，
 * 没有继承也没有回退。格子里有两个字段：`url` 是模块地址，`rewrite` 是路径前缀改写规则。
 * `resolveEndpointBaseUrl` 只读其中恰好一个。
 *
 * **本切片只写库里的东西。** 「当前环境」是客户端的实时状态，不落库，它由 `bootstrap-slice`
 * 的 `setCurrentEnvironment` 管。
 *
 * `deleteEnvironment` 会与自身键同一次原子 `set()` 里写别人拥有的键
 * （`activeEnvironmentId`）；拆成多次调用会改变 IPC 失败时回滚的内容。
 */
import type { StateCreator } from 'zustand';
import type { Environment, EnvWrite, ModuleBaseUrl, RewriteRule } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { pickDefaultEnvironment } from '@/lib/env-default';
import type { AppState } from '../app-store';
import { invokeWithRollback } from './crud-helpers';
/**
 * 同一格子的写请求排队用的队列。键是 `环境 id:模块 id`。
 *
 * 见 `setEnvironmentModuleBaseUrl` 里那几行：排队的是**请求**，乐观更新照旧立刻做。
 */
const cellWriteQueue = new Map<string, Promise<unknown>>();
/** 这一格最新的一次写的序号：回滚时只有它才回滚。 */
const latestCellWrite = new Map<string, number>();
/** 每一次写取一个号。 */
let cellWriteSeq = 0;

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
   * `url` 是模块地址，`rewrite` 是路径前缀改写规则。两个字段各自独立：
   * `null` = 该字段保持原值；起点与目标都空白的规则 = 清空规则。两个字段都清空时这一格整个
   * 消失。
   *
   * 每个格子都是独立取值——清一个不能碰到它的邻居。
   */
  setEnvironmentModuleBaseUrl: (
    envId: string,
    moduleId: string,
    url: string | null,
    rewrite: RewriteRule | null,
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

    // 乐观的单步交换：旧键消失与新键出现发生在同一次 state 写入里，故任何中间渲染都不会
    // 显示「变量不见了」。值跟着键走。
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
      // 一条命令、一个事务。拆成两次 `environments_set_var` 会在两次调用之间让变量处于已删除
      // 状态，而第二次调用失败时下面的回滚只还原了 UI，后端却已经把删除提交了。
      const updated = await invokeCmd<Environment>(COMMANDS.environmentsRenameVar, {
        envId,
        fromKey,
        toKey,
      });
      // 后端回显的是改名后权威的环境，故另一个窗口的并发编辑不会被这份乐观猜测覆盖。
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
  setEnvironmentModuleBaseUrl: (envId, moduleId, url, rewrite) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return Promise.resolve();
    // 格子里两个字段各自独立：`null` 保持原值。起点与目标都空白的规则落成 `null`（没有规则），
    // 与后端 `db::environments::set_module_base_url` 同一套。
    const cell = prev.moduleBaseUrls[moduleId];
    const nextCell: ModuleBaseUrl = {
      url: url === null ? (cell?.url ?? '') : url,
      rewrite:
        rewrite === null
          ? (cell?.rewrite ?? null)
          : rewrite.from.trim() === '' && rewrite.to.trim() === ''
            ? null
            : rewrite,
    };
    const nextModuleBaseUrls = { ...prev.moduleBaseUrls };
    // 两个字段都空了就把这一格删掉，免得已清空的环境还留着一个空的 `moduleId → {}` 外壳，
    // 在任何按键比对环境对象的 UI 里都读作「这里配了地址」。
    if (nextCell.url === '' && nextCell.rewrite === null) {
      delete nextModuleBaseUrls[moduleId];
    } else {
      nextModuleBaseUrls[moduleId] = nextCell;
    }
    const optimistic: Environment = { ...prev, moduleBaseUrls: nextModuleBaseUrls };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    // 同一格子的写请求按提交的先后顺序一个接一个发出去：设置面板的路径改写那一列有两个
    // 输入框，用户先改起点再改目标就是两次写。并发发出时服务器可能先把后提交的那条处理掉，
    // SQLite 里就留下旧的那条规则——界面显示的是新规则，请求却按旧规则改写。
    //
    // 排队的是**请求**：乐观更新照旧立刻做，输入框才不会卡。
    const key = `${envId}:${moduleId}`;
    const seq = ++cellWriteSeq;
    latestCellWrite.set(key, seq);
    const previous = cellWriteQueue.get(key) ?? Promise.resolve();
    const write = previous.then(() =>
      invokeWithRollback(
        'environments_set_module_baseurl',
        () =>
          invokeCmd<Environment>(COMMANDS.environmentsSetModuleBaseurl, {
            envId,
            moduleId,
            url,
            rewrite,
          }),
        () => {
          // 只有这一格**最新**的那次写失败时才回滚：更早的那次失败时，后来的写已经把界面
          // 更新过了，回滚会把用户刚改的内容一起抹掉。
          if (latestCellWrite.get(key) !== seq) return;
          set((s) => ({
            environments: s.environments.map((env) => (env.id === envId ? prev : env)),
          }));
        },
      ),
    );
    cellWriteQueue.set(key, write);
    return write.then(() => undefined);
  },
  deleteEnvironment: async (envId) => {
    const snapshot = {
      environments: get().environments,
      activeEnvironmentId: get().activeEnvironmentId,
    };
    set((s) => {
      const nextEnvs = s.environments.filter((env) => env.id !== envId);
      // 当前环境正好是刚删掉的那个时，回落到这个项目的默认环境（最早创建的那个）。项目里一个
      // 环境都不剩时置空（「未选」哨兵），顶栏触发器回落到 '—'。
      //
      // 端点不再绑定环境，故这里不必动 `endpointDetails`——删掉一个环境不改变任何接口的配置，
      // 接口读的是客户端当前的「当前环境」。
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
