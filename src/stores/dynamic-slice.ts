/**
 * 动态变量切片 —— 项目内的「变量名 → 数据源 → 抽取规则 → 缓存时长」声明，以及它们
 * 当前的取值。
 *
 * 定义**挂在项目里**：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。故这个
 * 切片处处带项目维度——取值桶就是按项目分的（`dynamicValues[projectId][name]`），
 * 否则切项目时上一个项目的同名值会被下一个项目读到，那正是这次要改掉的行为。
 *
 * 状态分两半：
 *
 *   - `dynamicVariables`：已加载的定义列表。每条定义自带它的归属项目，读者按项目过滤。
 *     列表命令一次带回定义 + 当前值 + 缓存状态，面板直接渲染它，不必再拼。
 *   - `dynamicValues`：**项目 id → 变量名 → 当前值**。取数用的环境由定义自己决定
 *     （点名的那个，或数据源接口所属项目的第一个可用环境），故同一个名字在一个项目里
 *     只有一个当前值——不按环境分桶，但要按项目分桶。
 *
 * 取数在**渲染进程**发起（`resolveDynamicVariables`），因为变量的值必须在
 * `prepareRequest` 替换 `{{var}}` 之前就到手——替换一旦发生，变量池里就没有同名变量
 * 的概念了。合并规则见 `src/lib/dynamic-vars.ts` 的 `mergePool`。
 */
import type { StateCreator } from 'zustand';
import type {
  DynamicError,
  DynamicValueStatus,
  DynamicVariable,
  DynamicVariableView,
  ValueMode,
} from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from './app-store';

/** 新建一条定义时要填的字段。 */
export interface CreateDynamicVariableInput {
  /** 归属项目。定义挂在项目里，故它必填。 */
  projectId: string;
  name: string;
  description?: string;
  sourceEndpointId?: string | null;
  sourceInlineJson?: string | null;
  /** 取数用的环境。空 = 数据源接口所属项目的第一个可用环境。 */
  sourceEnvironmentId?: string | null;
  extractPath?: string;
  /**
   * 处理脚本。抽取之后跑，用来做 JSONPath 表达不了的加工。脚本拿得到抽取结果
   * （`pm.data`）与原始响应（`pm.response`），用 `return` 给出最终值。
   */
  transformScript?: string | null;
  valueMode?: ValueMode;
  ttlSeconds?: number;
  staleFallback?: boolean;
  enabled?: boolean;
}

/** 状态部分。 */
export interface DynamicSliceState {
  /** 已加载的定义（带当前值与缓存状态）。每条自带归属项目，读者按项目过滤。 */
  dynamicVariables: DynamicVariableView[];
  /** 项目 id → 变量名 → 当前值。 */
  dynamicValues: Record<string, Record<string, DynamicValueStatus>>;
  /** 项目 id → 变量名 → 最近一次取数失败的原因。取数成功时该条目被清掉。 */
  dynamicErrors: Record<string, Record<string, DynamicError>>;
  /** 正在取数。界面上据此显示「正在取数…」。 */
  dynamicResolving: boolean;
  /** 最近一次取数或写操作失败的原因。 */
  dynamicError: string | null;
}

/** action 部分。 */
export interface DynamicSliceActions {
  /**
   * 载入定义。
   *
   * 给了 `projectId` 就只载入那个项目的；不给就是跨项目那一路（列出全部）。
   * 只读缓存行，**不取数**。
   */
  loadDynamicVariables: (projectId?: string) => Promise<void>;
  /** 新建一条定义，返回它的 id（失败返回 null）。 */
  createDynamicVariable: (input: CreateDynamicVariableInput) => Promise<string | null>;
  /** 改一条定义。成功返回 true，失败返回 false（原因在 `dynamicError` 里）。 */
  updateDynamicVariable: (id: string, patch: Partial<DynamicVariable>) => Promise<boolean>;
  /** 删一条定义。 */
  deleteDynamicVariable: (id: string) => Promise<void>;
  /**
   * 解析一批名字，返回它们的值与错误。
   *
   * `projectId` 决定这些名字在哪个范围里找：给了就只认那个项目里的定义（请求发送走
   * 的就是这条），不给就是跨项目取值（同名定义出现在多个项目里时按歧义处理，不猜）。
   *
   * 值同时写进 store，故请求编辑器下一次 `prepareRequest` 会用上它们。TTL 内命中缓存
   * 时后端不发取数请求，故这一步通常是廉价的。
   */
  resolveDynamicVariables: (
    names: string[],
    projectId?: string,
    force?: boolean,
  ) => Promise<{ values: Record<string, string>; errors: DynamicError[] }>;
  /** 写覆盖值。`value` 为 null 表示清除。 */
  setDynamicOverride: (
    id: string,
    value: string | null,
    options?: { expiresAt?: number | null; forever?: boolean },
  ) => Promise<void>;
  /** 强制重新取一次数。 */
  refreshDynamicVariable: (id: string) => Promise<void>;
  /**
   * 把一个变量名从取值桶里剔除。
   *
   * 停用或删除之后调用它：留着旧值就等于停用之后仍然在替换。
   */
  dropDynamicValue: (projectId: string, name: string) => void;
}

/** 切片。 */
export type DynamicSlice = DynamicSliceState & DynamicSliceActions;

export const createDynamicSlice: StateCreator<AppState, [], [], DynamicSlice> = (set, get) => ({
  dynamicVariables: [],
  dynamicValues: {},
  dynamicErrors: {},
  dynamicResolving: false,
  dynamicError: null,

  /**
   * 把一个变量名从取值桶里剔除。
   *
   * 停用或删除之后，它残留的旧值不该留着：请求编辑器把桶里的值并进变量池，
   * 留着就等于停用之后仍然在替换。
   *
   * 桶按项目分，故要连项目一起给：同名变量在别的项目里还活着，那一份不能被一起删掉。
   */
  dropDynamicValue: (projectId, name) =>
    set((s) => {
      const values = { ...s.dynamicValues };
      const errors = { ...s.dynamicErrors };
      const projectValues = values[projectId];
      const projectErrors = errors[projectId];
      if (projectValues) {
        const next = { ...projectValues };
        delete next[name];
        if (Object.keys(next).length === 0) delete values[projectId];
        else values[projectId] = next;
      }
      if (projectErrors) {
        const next = { ...projectErrors };
        delete next[name];
        if (Object.keys(next).length === 0) delete errors[projectId];
        else errors[projectId] = next;
      }
      return { dynamicValues: values, dynamicErrors: errors };
    }),

  loadDynamicVariables: async (projectId) => {
    try {
      const views = await invokeCmd<DynamicVariableView[]>(COMMANDS.dynamicVariablesList, {
        // 空串与缺席同义：不点名项目就是跨项目那一路（列出全部）。
        projectId: projectId || null,
      });
      set({ dynamicVariables: views, dynamicError: null });
    } catch (e) {
      console.error('[store] dynamic_variables_list failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  createDynamicVariable: async (input) => {
    try {
      const created = await invokeCmd<DynamicVariable>(COMMANDS.dynamicVariablesCreate, {
        projectId: input.projectId,
        name: input.name,
        description: input.description ?? null,
        sourceEndpointId: input.sourceEndpointId ?? null,
        sourceInlineJson: input.sourceInlineJson ?? null,
        sourceEnvironmentId: input.sourceEnvironmentId ?? null,
        extractPath: input.extractPath ?? null,
        transformScript: input.transformScript ?? null,
        valueMode: input.valueMode ?? null,
        ttlSeconds: input.ttlSeconds ?? null,
        staleFallback: input.staleFallback ?? null,
        enabled: input.enabled ?? null,
      });
      // 列表随即重载：面板要显示的当前值与缓存状态都在列表里。
      await get().loadDynamicVariables(input.projectId);
      return created.id;
    } catch (e) {
      console.error('[store] dynamic_variables_create failed', e);
      set({ dynamicError: messageOf(e) });
      return null;
    }
  },

  updateDynamicVariable: async (id, patch) => {
    // 改之前的名字：改名之后旧名下的值与错误都不再有意义。
    const before = get().dynamicVariables.find((v) => v.id === id);
    try {
      await invokeCmd<DynamicVariable>(COMMANDS.dynamicVariablesUpdate, { id, patch });
      if (before) get().dropDynamicValue(before.projectId ?? '', before.name);
      await get().loadDynamicVariables(scopeOf(get(), id));
      return true;
    } catch (e) {
      console.error('[store] dynamic_variables_update failed', e);
      set({ dynamicError: messageOf(e) });
      return false;
    }
  },

  deleteDynamicVariable: async (id) => {
    const before = get().dynamicVariables.find((v) => v.id === id);
    try {
      await invokeCmd<void>(COMMANDS.dynamicVariablesDelete, { id });
      if (before) get().dropDynamicValue(before.projectId ?? '', before.name);
      await get().loadDynamicVariables(scopeOf(get(), id));
    } catch (e) {
      console.error('[store] dynamic_variables_delete failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  resolveDynamicVariables: async (names, projectId, force) => {
    if (names.length === 0) return { values: {}, errors: [] };
    set({ dynamicResolving: true });
    try {
      const resolution = await invokeCmd<{
        values: Record<string, string>;
        statuses: DynamicValueStatus[];
        errors: DynamicError[];
      }>(COMMANDS.dynamicVariablesResolve, {
        names,
        // 空串与缺席同义：不点名项目就是跨项目取值。
        projectId: projectId || null,
        force: force ?? false,
      });
      // 值写进 store：请求编辑器下一次 `prepareRequest` 会用上它们。
      //
      // 桶按项目分：跨项目取值（不点名项目）时那些值没有项目可落，故归到空串那一格——
      // 那一格不参与任何项目的请求替换。
      const scope = projectId ?? '';
      set((s) => {
        const bucket = { ...(s.dynamicValues[scope] ?? {}) };
        for (const status of resolution.statuses) {
          bucket[status.name] = status;
        }
        // 失败原因按变量名记下来，面板要能说清「为什么这个变量没有值」。
        const errors = { ...(s.dynamicErrors[scope] ?? {}) };
        for (const error of resolution.errors) {
          errors[error.name] = error;
        }
        for (const status of resolution.statuses) {
          if (!status.error) delete errors[status.name];
        }
        return {
          dynamicValues: { ...s.dynamicValues, [scope]: bucket },
          dynamicErrors: { ...s.dynamicErrors, [scope]: errors },
        };
      });
      return { values: resolution.values, errors: resolution.errors };
    } catch (e) {
      console.error('[store] dynamic_variables_resolve failed', e);
      set({ dynamicError: messageOf(e) });
      return { values: {}, errors: [] };
    } finally {
      set({ dynamicResolving: false });
    }
  },

  setDynamicOverride: async (id, value, options) => {
    try {
      await invokeCmd(COMMANDS.dynamicVariablesSetOverride, {
        id,
        value,
        expiresAt: options?.expiresAt ?? null,
        forever: options?.forever ?? false,
      });
      // 覆盖值在手就不用再取数，上一次取数失败的原因随之作废。
      const definition = get().dynamicVariables.find((v) => v.id === id);
      const name = definition?.name;
      const scope = definition?.projectId ?? '';
      if (name) {
        set((s) => {
          const errors = { ...(s.dynamicErrors[scope] ?? {}) };
          delete errors[name];
          return { dynamicErrors: { ...s.dynamicErrors, [scope]: errors } };
        });
      }
      await get().loadDynamicVariables(scopeOf(get(), id));
    } catch (e) {
      console.error('[store] dynamic_variables_set_override failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  refreshDynamicVariable: async (id) => {
    set({ dynamicResolving: true });
    try {
      const status = await invokeCmd<DynamicValueStatus>(COMMANDS.dynamicVariablesRefresh, { id });
      // 这次强制重取的结果（值与失败原因）写进 store，面板立刻能显示。
      const scope = get().dynamicVariables.find((v) => v.id === id)?.projectId ?? '';
      set((s) => {
        const bucket = { ...(s.dynamicValues[scope] ?? {}) };
        if (status.name) bucket[status.name] = status;
        const errors = { ...(s.dynamicErrors[scope] ?? {}) };
        if (status.error) {
          errors[status.name] = status.error;
        } else {
          delete errors[status.name];
        }
        return {
          dynamicValues: { ...s.dynamicValues, [scope]: bucket },
          dynamicErrors: { ...s.dynamicErrors, [scope]: errors },
        };
      });
      await get().loadDynamicVariables(scopeOf(get(), id));
    } catch (e) {
      console.error('[store] dynamic_variables_refresh failed', e);
      set({ dynamicError: messageOf(e) });
    } finally {
      set({ dynamicResolving: false });
    }
  },
});

/**
 * 一条定义属于哪个项目 —— 也就是「重载列表时该列哪个项目」。
 *
 * 优先用定义自己的归属项目；定义不在已加载的列表里（于是无从查它的归属）时退回
 * 当前活动项目 —— 那是用户正在看的那个，也是面板与请求页要显示的那一个。
 */
function scopeOf(state: AppState, id: string): string | undefined {
  const definition = state.dynamicVariables.find((v) => v.id === id);
  return definition?.projectId ?? (state.activeProjectId || undefined);
}

/** 错误消息。`invokeCmd` 抛的是 Error，但外部来源的载荷未必是。 */
function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
