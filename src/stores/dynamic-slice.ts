/**
 * 动态变量切片 —— 项目级的「变量名 → 数据源 → 抽取规则 → 缓存时长」声明，以及它们
 * 当前的取值。
 *
 * 状态分两半：
 *
 *   - `dynamicVariables`：本项目的定义列表。列表命令一次带回定义 + 当前值 + 缓存
 *     状态，面板直接渲染它，不必再拼。
 *   - `dynamicValues`：按环境 id 分桶的当前值。缓存键含环境——同一个定义在不同环境
 *     取到的值不同（数据源端点的 base URL 随环境变，测试环境与生产环境的站点清单
 *     本来就不是一回事）。
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
  name: string;
  description?: string;
  sourceEndpointId?: string | null;
  sourceInlineJson?: string | null;
  extractPath?: string;
  valueMode?: ValueMode;
  ttlSeconds?: number;
  staleFallback?: boolean;
  enabled?: boolean;
}

/** 状态部分。 */
export interface DynamicSliceState {
  /** 本项目的定义列表（带当前值与缓存状态）。 */
  dynamicVariables: DynamicVariableView[];
  /** 环境 id → 变量名 → 当前值。 */
  dynamicValues: Record<string, Record<string, DynamicValueStatus>>;
  /** 正在取数。界面上据此显示「正在取数…」。 */
  dynamicResolving: boolean;
  /** 最近一次取数或写操作失败的原因。 */
  dynamicError: string | null;
}

/** action 部分。 */
export interface DynamicSliceActions {
  /** 载入本项目的定义。 */
  loadDynamicVariables: (projectId: string, environmentId?: string) => Promise<void>;
  /** 新建一条定义，返回它的 id（失败返回 null）。 */
  createDynamicVariable: (
    projectId: string,
    input: CreateDynamicVariableInput,
  ) => Promise<string | null>;
  /** 改一条定义。 */
  updateDynamicVariable: (id: string, patch: Partial<DynamicVariable>) => Promise<void>;
  /** 删一条定义。 */
  deleteDynamicVariable: (id: string) => Promise<void>;
  /**
   * 解析一批名字，返回它们的值与错误。
   *
   * 值同时写进 store，故请求编辑器下一次 `prepareRequest` 会用上它们。TTL 内命中缓存
   * 时后端不发取数请求，故这一步通常是廉价的。
   */
  resolveDynamicVariables: (
    projectId: string,
    names: string[],
    environmentId?: string,
    force?: boolean,
  ) => Promise<{ values: Record<string, string>; errors: DynamicError[] }>;
  /** 写覆盖值。`value` 为 null 表示清除。 */
  setDynamicOverride: (
    id: string,
    value: string | null,
    options?: { environmentId?: string; expiresAt?: number | null; forever?: boolean },
  ) => Promise<void>;
  /** 强制重新取一次数。 */
  refreshDynamicVariable: (projectId: string, id: string, environmentId?: string) => Promise<void>;
}

/** 切片。 */
export type DynamicSlice = DynamicSliceState & DynamicSliceActions;

export const createDynamicSlice: StateCreator<AppState, [], [], DynamicSlice> = (set, get) => ({
  dynamicVariables: [],
  dynamicValues: {},
  dynamicResolving: false,
  dynamicError: null,

  loadDynamicVariables: async (projectId, environmentId) => {
    try {
      const views = await invokeCmd<DynamicVariableView[]>(COMMANDS.dynamicVariablesList, {
        projectId,
        environmentId: environmentId ?? null,
      });
      set({ dynamicVariables: views, dynamicError: null });
    } catch (e) {
      console.error('[store] dynamic_variables_list failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  createDynamicVariable: async (projectId, input) => {
    try {
      const created = await invokeCmd<DynamicVariable>(COMMANDS.dynamicVariablesCreate, {
        projectId,
        name: input.name,
        description: input.description ?? null,
        sourceEndpointId: input.sourceEndpointId ?? null,
        sourceInlineJson: input.sourceInlineJson ?? null,
        extractPath: input.extractPath ?? null,
        valueMode: input.valueMode ?? null,
        ttlSeconds: input.ttlSeconds ?? null,
        staleFallback: input.staleFallback ?? null,
        enabled: input.enabled ?? null,
      });
      // 列表随即重载：面板要显示的当前值与缓存状态都在列表里。
      await get().loadDynamicVariables(projectId, get().activeEnvironmentId || undefined);
      return created.id;
    } catch (e) {
      console.error('[store] dynamic_variables_create failed', e);
      set({ dynamicError: messageOf(e) });
      return null;
    }
  },

  updateDynamicVariable: async (id, patch) => {
    const projectId = get().activeProjectId;
    try {
      await invokeCmd<DynamicVariable>(COMMANDS.dynamicVariablesUpdate, { id, patch });
      await get().loadDynamicVariables(projectId, get().activeEnvironmentId || undefined);
    } catch (e) {
      console.error('[store] dynamic_variables_update failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  deleteDynamicVariable: async (id) => {
    const projectId = get().activeProjectId;
    try {
      await invokeCmd<void>(COMMANDS.dynamicVariablesDelete, { id });
      await get().loadDynamicVariables(projectId, get().activeEnvironmentId || undefined);
    } catch (e) {
      console.error('[store] dynamic_variables_delete failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  resolveDynamicVariables: async (projectId, names, environmentId, force) => {
    if (names.length === 0) return { values: {}, errors: [] };
    const envId = environmentId ?? get().activeEnvironmentId ?? '';
    set({ dynamicResolving: true });
    try {
      const resolution = await invokeCmd<{
        values: Record<string, string>;
        statuses: DynamicValueStatus[];
        errors: DynamicError[];
      }>(COMMANDS.dynamicVariablesResolve, {
        projectId,
        names,
        environmentId: envId || null,
        force: force ?? false,
      });
      // 值写进 store：请求编辑器下一次 `prepareRequest` 会用上它们。
      set((s) => {
        const bucket = { ...(s.dynamicValues[envId] ?? {}) };
        for (const status of resolution.statuses) {
          bucket[status.name] = status;
        }
        return { dynamicValues: { ...s.dynamicValues, [envId]: bucket } };
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
    const projectId = get().activeProjectId;
    const environmentId = options?.environmentId ?? get().activeEnvironmentId ?? '';
    try {
      await invokeCmd(COMMANDS.dynamicVariablesSetOverride, {
        id,
        value,
        environmentId: environmentId || null,
        expiresAt: options?.expiresAt ?? null,
        forever: options?.forever ?? false,
      });
      await get().loadDynamicVariables(projectId, environmentId || undefined);
    } catch (e) {
      console.error('[store] dynamic_variables_set_override failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  refreshDynamicVariable: async (projectId, id, environmentId) => {
    const envId = environmentId ?? get().activeEnvironmentId ?? '';
    set({ dynamicResolving: true });
    try {
      await invokeCmd(COMMANDS.dynamicVariablesRefresh, {
        projectId,
        id,
        environmentId: envId || null,
      });
      await get().loadDynamicVariables(projectId, envId || undefined);
    } catch (e) {
      console.error('[store] dynamic_variables_refresh failed', e);
      set({ dynamicError: messageOf(e) });
    } finally {
      set({ dynamicResolving: false });
    }
  },
});

/** 错误消息。`invokeCmd` 抛的是 Error，但外部来源的载荷未必是。 */
function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
