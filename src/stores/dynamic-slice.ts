/**
 * 动态变量切片 —— 全局作用域的「变量名 → 数据源 → 抽取规则 → 缓存时长」声明，以及它们
 * 当前的取值。
 *
 * 定义**不属于任何项目**：任何项目的端点都能引用同一条定义，故这个切片里没有项目维度。
 * 变量名在整个工作区里唯一。
 *
 * 状态分两半：
 *
 *   - `dynamicVariables`：全部定义的列表。列表命令一次带回定义 + 当前值 + 缓存状态，
 *     面板直接渲染它，不必再拼。
 *   - `dynamicValues`：变量名 → 当前值。取数用的环境由定义自己决定（点名的那个，或数据源
 *     接口所属项目的第一个可用环境），故同一个名字只有一个当前值——不按环境分桶。
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
  /** 取数用的环境。空 = 数据源接口所属项目的第一个可用环境。 */
  sourceEnvironmentId?: string | null;
  /** 取数用的服务。空 = 数据源接口所属项目的默认服务。 */
  sourceBaseUrlDefinitionId?: string | null;
  extractPath?: string;
  valueMode?: ValueMode;
  ttlSeconds?: number;
  staleFallback?: boolean;
  enabled?: boolean;
}

/** 状态部分。 */
export interface DynamicSliceState {
  /** 全部定义（带当前值与缓存状态）。 */
  dynamicVariables: DynamicVariableView[];
  /** 变量名 → 当前值。 */
  dynamicValues: Record<string, DynamicValueStatus>;
  /** 变量名 → 最近一次取数失败的原因。取数成功时该条目被清掉。 */
  dynamicErrors: Record<string, DynamicError>;
  /** 正在取数。界面上据此显示「正在取数…」。 */
  dynamicResolving: boolean;
  /** 最近一次取数或写操作失败的原因。 */
  dynamicError: string | null;
}

/** action 部分。 */
export interface DynamicSliceActions {
  /**
   * 载入全部定义。
   *
   * `includeReferences` 决定要不要算「哪些端点引用了它」——算它要扫一遍全部端点，故
   * 只要当前值与缓存状态的调用方（请求编辑器每次挂载都调一次）传 `false`。
   *
   * 不带引用关系的加载**不会清空**已有的引用关系：面板算过一次之后，请求页的那次便宜
   * 加载不该把面板上的「被引用」抹掉。
   */
  loadDynamicVariables: (options?: { includeReferences?: boolean }) => Promise<void>;
  /** 新建一条定义，返回它的 id（失败返回 null）。 */
  createDynamicVariable: (input: CreateDynamicVariableInput) => Promise<string | null>;
  /** 改一条定义。成功返回 true，失败返回 false（原因在 `dynamicError` 里）。 */
  updateDynamicVariable: (id: string, patch: Partial<DynamicVariable>) => Promise<boolean>;
  /** 删一条定义。 */
  deleteDynamicVariable: (id: string) => Promise<void>;
  /**
   * 解析一批名字，返回它们的值与错误。
   *
   * 值同时写进 store，故请求编辑器下一次 `prepareRequest` 会用上它们。TTL 内命中缓存
   * 时后端不发取数请求，故这一步通常是廉价的。
   */
  resolveDynamicVariables: (
    names: string[],
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
  dropDynamicValue: (name: string) => void;
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
   */
  dropDynamicValue: (name) =>
    set((s) => {
      const values = { ...s.dynamicValues };
      delete values[name];
      const errors = { ...s.dynamicErrors };
      delete errors[name];
      return { dynamicValues: values, dynamicErrors: errors };
    }),

  loadDynamicVariables: async (options) => {
    const includeReferences = options?.includeReferences ?? true;
    try {
      const views = await invokeCmd<DynamicVariableView[]>(COMMANDS.dynamicVariablesList, {
        includeReferences,
      });
      set((s) => ({
        dynamicVariables: includeReferences
          ? views
          : keepKnownReferences(s.dynamicVariables, views),
        dynamicError: null,
      }));
    } catch (e) {
      console.error('[store] dynamic_variables_list failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  createDynamicVariable: async (input) => {
    try {
      const created = await invokeCmd<DynamicVariable>(COMMANDS.dynamicVariablesCreate, {
        name: input.name,
        description: input.description ?? null,
        sourceEndpointId: input.sourceEndpointId ?? null,
        sourceInlineJson: input.sourceInlineJson ?? null,
        sourceEnvironmentId: input.sourceEnvironmentId ?? null,
        sourceBaseUrlDefinitionId: input.sourceBaseUrlDefinitionId ?? null,
        extractPath: input.extractPath ?? null,
        valueMode: input.valueMode ?? null,
        ttlSeconds: input.ttlSeconds ?? null,
        staleFallback: input.staleFallback ?? null,
        enabled: input.enabled ?? null,
      });
      // 列表随即重载：面板要显示的当前值与缓存状态都在列表里。
      await get().loadDynamicVariables();
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
      if (before) get().dropDynamicValue(before.name);
      await get().loadDynamicVariables();
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
      if (before) get().dropDynamicValue(before.name);
      await get().loadDynamicVariables();
    } catch (e) {
      console.error('[store] dynamic_variables_delete failed', e);
      set({ dynamicError: messageOf(e) });
    }
  },

  resolveDynamicVariables: async (names, force) => {
    if (names.length === 0) return { values: {}, errors: [] };
    set({ dynamicResolving: true });
    try {
      const resolution = await invokeCmd<{
        values: Record<string, string>;
        statuses: DynamicValueStatus[];
        errors: DynamicError[];
      }>(COMMANDS.dynamicVariablesResolve, {
        names,
        force: force ?? false,
      });
      // 值写进 store：请求编辑器下一次 `prepareRequest` 会用上它们。
      set((s) => {
        const bucket = { ...s.dynamicValues };
        for (const status of resolution.statuses) {
          bucket[status.name] = status;
        }
        // 失败原因按变量名记下来，面板要能说清「为什么这个变量没有值」。
        const errors = { ...s.dynamicErrors };
        for (const error of resolution.errors) {
          errors[error.name] = error;
        }
        for (const status of resolution.statuses) {
          if (!status.error) delete errors[status.name];
        }
        return { dynamicValues: bucket, dynamicErrors: errors };
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
      const name = get().dynamicVariables.find((v) => v.id === id)?.name;
      if (name) {
        set((s) => {
          const errors = { ...s.dynamicErrors };
          delete errors[name];
          return { dynamicErrors: errors };
        });
      }
      await get().loadDynamicVariables();
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
      set((s) => {
        const bucket = { ...s.dynamicValues };
        if (status.name) bucket[status.name] = status;
        const errors = { ...s.dynamicErrors };
        if (status.error) {
          errors[status.name] = status.error;
        } else {
          delete errors[status.name];
        }
        return { dynamicValues: bucket, dynamicErrors: errors };
      });
      await get().loadDynamicVariables();
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

/**
 * 把上一次已知的引用关系搬到新载入的列表上。
 *
 * 不带引用关系的加载拿回来的 `referencedBy` 一定是空的：后端没算。若直接覆盖，面板上
 * 已经算出来的「被引用」就被这次便宜加载抹掉了。故按 id 把旧值搬过来。
 */
function keepKnownReferences(
  previous: DynamicVariableView[],
  incoming: DynamicVariableView[],
): DynamicVariableView[] {
  const known = new Map(previous.map((v) => [v.id, v.referencedBy]));
  return incoming.map((view) => {
    const refs = known.get(view.id);
    return refs && refs.length > 0 ? { ...view, referencedBy: refs } : view;
  });
}
