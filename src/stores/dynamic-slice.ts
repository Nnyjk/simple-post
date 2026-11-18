/**
 * 动态变量切片 —— 项目内的「变量名 → 数据源 → 抽取规则 → 缓存时长」声明，以及它们当前的
 * 取值。
 *
 * 定义**挂在项目里**：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。故这个切片
 * 处处带项目维度——取值桶就是按项目分的（`dynamicValues[projectId][name]`），否则切项目时
 * 上一个项目的同名值会被下一个项目读到。
 *
 * 状态分两半：
 *
 *   - `dynamicVariables`：已加载的定义列表。每条定义自带它的归属项目，读者按项目过滤。
 *     列表命令一次带回定义 + 当前值 + 缓存状态，面板直接渲染它，不必再拼。
 *   - `dynamicValues`：**项目 id → 变量名 → 当前值**。取数用的环境由定义决定：点名的那个，
 *     或（定义没点名时）**调用方此刻选中的环境**——也就是顶栏那个 `activeEnvironmentId`。
 *     故同一个名字在一个项目里只有一份当前值：那一份是「当前环境下」的值，不按环境分桶，
 *     但要按项目分桶。
 *
 * 因为取数环境随用户切环境而变，切片里的每条命令都把 `activeEnvironmentId` 一并送上去
 * （见 `callerEnvironmentId`）。**这是切片自己的状态，不由调用方传**：环境只有一份真相，
 * 让每个调用点各读一次，迟早有人读到旧的。
 *
 * 取数在**渲染进程**发起（`resolveDynamicVariables`），因为变量的值必须在 `prepareRequest`
 * 替换 `{{var}}` 之前就到手——替换一旦发生，变量池里就没有同名变量的概念了。合并规则见
 * `src/lib/dynamic-vars.ts` 的 `mergePool`。
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
import { errorText } from '@/lib/error-text';
import type { AppState } from './app-store';

/** 新建一条定义时要填的字段。 */
export interface CreateDynamicVariableInput {
  /** 归属项目。定义挂在项目里，故它必填。 */
  projectId: string;
  name: string;
  description?: string;
  sourceEndpointId?: string | null;
  sourceInlineJson?: string | null;
  /** 取数用的环境。空 = 继承调用方此刻选中的环境（见 `callerEnvironmentId`）。 */
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

/** 数据源的两种形态。形态是草稿里的一格：「当前选中的接口」推不出它，切回内联时接口 id 还在。 */
export type DynamicSourceKind = 'endpoint' | 'inline';

/**
 * 编辑中的动态变量草稿 —— 面板与表单的全部输入格。
 *
 * 放在 store 而不是组件的 `useState` 里，因为动态变量是项目内的一页：切到别的标签时
 * `WorkspaceShell` 会连面板一起卸载，组件 state 随之蒸发，用户填了一半的表单就没了。
 * 草稿按**项目 id** 分桶（`dynamicDrafts[projectId]`），面板与表单直接读写它。
 *
 * 只装表单内容：预览响应、busy、保存错误是瞬时状态，不进来——切回来少一次预览可以接受，
 * 少半张表单不行。
 */
export interface DynamicVariableDraft {
  /** 面板的「新建定义」表单开着没有。与 `editingId` 互斥：一屏只挂一张表。 */
  creating: boolean;
  /** 正在编辑哪条定义；`null` = 没在编辑。 */
  editingId: string | null;
  name: string;
  description: string;
  sourceKind: DynamicSourceKind;
  sourceEndpointId: string;
  sourceProjectId: string;
  /** 数据源接口的名字。树上点一下就有，不必再查一次树。 */
  sourceEndpointName: string;
  sourceInlineJson: string;
  /** 取数用的环境。空串 = 继承当前环境（发送请求时用户选中的那个）。 */
  environmentId: string;
  extractPath: string;
  transformScript: string;
  valueMode: ValueMode;
  /** 输入框里的原文而不是数字：用户清空那一格时，这个「空」得留得住。 */
  ttlSeconds: string;
  staleFallback: boolean;
  enabled: boolean;
}

/** 表单逐格写的那部分。`creating` / `editingId` 由面板的两个按钮决定，表单不碰。 */
export type DynamicVariableDraftFields = Omit<DynamicVariableDraft, 'creating' | 'editingId'>;

/** 一份空白的新建草稿。 */
export function emptyDynamicVariableDraft(): DynamicVariableDraft {
  return {
    creating: true,
    editingId: null,
    name: '',
    description: '',
    sourceKind: 'inline',
    sourceEndpointId: '',
    sourceProjectId: '',
    sourceEndpointName: '',
    sourceInlineJson: '',
    environmentId: '',
    extractPath: '$',
    transformScript: '',
    valueMode: 'auto',
    ttlSeconds: '60',
    staleFallback: false,
    enabled: true,
  };
}

/**
 * 从一条已存定义垫出编辑草稿。
 *
 * 「数据源接口已被删除」的定义落在**引用接口**形态：那条路是唯一能救它的（重新挑一个接口），
 * 落进内联形态会让用户以为要手写一份请求。
 */
export function draftFromDefinition(variable: DynamicVariableView): DynamicVariableDraft {
  const sourceDeleted = !variable.sourceEndpointId && !variable.sourceInlineJson;
  return {
    creating: false,
    editingId: variable.id,
    name: variable.name,
    description: variable.description ?? '',
    sourceKind: variable.sourceEndpointId || sourceDeleted ? 'endpoint' : 'inline',
    sourceEndpointId: variable.sourceEndpointId ?? '',
    sourceProjectId: variable.sourceProjectId ?? '',
    sourceEndpointName: variable.sourceEndpointName ?? '',
    sourceInlineJson: variable.sourceInlineJson ?? '',
    environmentId: variable.sourceEnvironmentId ?? '',
    extractPath: variable.extractPath,
    transformScript: variable.transformScript ?? '',
    valueMode: variable.valueMode,
    ttlSeconds: String(variable.ttlSeconds),
    staleFallback: variable.staleFallback,
    enabled: variable.enabled,
  };
}

/** 状态部分。 */
export interface DynamicSliceState {
  /** 已加载的定义（带当前值与缓存状态）。每条自带归属项目，读者按项目过滤。 */
  dynamicVariables: DynamicVariableView[];
  /**
   * 项目 id → 编辑中的草稿。切标签、切项目都不动它；保存成功、取消、被编辑的定义被删掉、
   * 那条标签被关掉时才清（清理点见 `DynamicSliceActions.clearDynamicVariableDraft`）。
   */
  dynamicDrafts: Record<string, DynamicVariableDraft>;
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
   * 打开「新建定义」表单。
   *
   * 这个项目已经开着一张新建表就沿用它的内容；开着的是编辑表则换掉——一屏只挂一张表，
   * 两张共用同一堆字段只会互相镜像。
   */
  beginCreateDynamicVariableDraft: (projectId: string) => void;
  /** 开始编辑某条定义。正在编辑的正是这一条就沿用，用户改了一半的内容不重置。 */
  beginEditDynamicVariableDraft: (variable: DynamicVariableView) => void;
  /** 改草稿里的格子（浅合并）。还没有草稿时先垫一份空白的。 */
  patchDynamicVariableDraft: (
    projectId: string,
    patch: Partial<DynamicVariableDraftFields>,
  ) => void;
  /**
   * 丢掉某个项目的草稿。四个清理点是：保存成功、点「取消」、被编辑的定义删掉、
   * 这个项目的动态变量标签关掉（`closeTab` / `pruneStaleTabs` / 批量关闭）或项目被删。
   * **切标签、切项目都不清**——那正是把草稿搬出组件树的原因。
   */
  clearDynamicVariableDraft: (projectId: string) => void;
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
   * `projectId` 决定这些名字在哪个范围里找：给了就只认那个项目里的定义（请求发送走的就是
   * 这条），不给就是跨项目取值（同名定义出现在多个项目里时按歧义处理，不猜）。
   *
   * 值同时写进 store，故请求编辑器下一次 `prepareRequest` 会用上它们。TTL 内命中缓存时后端
   * 不发取数请求，故这一步通常是廉价的。
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
  /** 把一个变量名从取值桶里剔除。停用或删除之后调用它：留着旧值就等于停用之后仍然在替换。 */
  dropDynamicValue: (projectId: string, name: string) => void;
}

/** 切片。 */
export type DynamicSlice = DynamicSliceState & DynamicSliceActions;

export const createDynamicSlice: StateCreator<AppState, [], [], DynamicSlice> = (set, get) => ({
  dynamicVariables: [],
  dynamicDrafts: {},
  dynamicValues: {},
  dynamicErrors: {},
  dynamicResolving: false,
  dynamicError: null,

  beginCreateDynamicVariableDraft: (projectId) =>
    set((s) => {
      // 已经开着一张新建表：沿用它的内容，只保证它确实开着。
      const draft = s.dynamicDrafts[projectId];
      if (draft?.creating) return {};
      return {
        dynamicDrafts: { ...s.dynamicDrafts, [projectId]: emptyDynamicVariableDraft() },
      };
    }),

  beginEditDynamicVariableDraft: (variable) =>
    set((s) => {
      const projectId = variable.projectId ?? '';
      // 正在编辑的就是这一条：保留用户改了一半的内容。
      if (s.dynamicDrafts[projectId]?.editingId === variable.id) return {};
      return {
        dynamicDrafts: { ...s.dynamicDrafts, [projectId]: draftFromDefinition(variable) },
      };
    }),

  patchDynamicVariableDraft: (projectId, patch) =>
    set((s) => {
      // 浅合并：一个项目一份草稿，改一格就换一个新对象，别的项目的引用原样保留（选择器靠
      // 引用比较，重建整张表会让没在编辑的项目的面板也重渲染一次）。
      //
      // 没有草稿时兜底的那份既不是新建表也不是编辑表：兜底若带 `creating: true`，一次没人
      // 开表单的 patch 就会在面板上凭空长出一张新建表。
      const draft = s.dynamicDrafts[projectId] ?? {
        ...emptyDynamicVariableDraft(),
        creating: false,
        editingId: null,
      };
      return { dynamicDrafts: { ...s.dynamicDrafts, [projectId]: { ...draft, ...patch } } };
    }),

  clearDynamicVariableDraft: (projectId) =>
    set((s) => {
      if (!(projectId in s.dynamicDrafts)) return {};
      const dynamicDrafts = { ...s.dynamicDrafts };
      delete dynamicDrafts[projectId];
      return { dynamicDrafts };
    }),

  /**
   * 把一个变量名从取值桶里剔除。停用或删除之后，它残留的旧值不该留着：请求编辑器把桶里的值
   * 并进变量池，留着就等于停用之后仍然在替换。
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
        // 列表带回的当前值是「在某个环境下的值」，故要看哪一格缓存得说清是哪个环境。
        environmentId: callerEnvironmentId(get()),
      });
      set({ dynamicVariables: views, dynamicError: null });
    } catch (e) {
      console.error('[store] dynamic_variables_list failed', e);
      set({ dynamicError: errorText(e) });
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
      set({ dynamicError: errorText(e) });
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
      set({ dynamicError: errorText(e) });
      return false;
    }
  },

  deleteDynamicVariable: async (id) => {
    const before = get().dynamicVariables.find((v) => v.id === id);
    try {
      await invokeCmd<void>(COMMANDS.dynamicVariablesDelete, { id });
      if (before) {
        get().dropDynamicValue(before.projectId ?? '', before.name);
        // 正在编辑的那条定义被删了：表单已经存不回去，草稿跟着走。
        const projectId = before.projectId ?? '';
        if (projectId !== '' && get().dynamicDrafts[projectId]?.editingId === id) {
          get().clearDynamicVariableDraft(projectId);
        }
      }
      await get().loadDynamicVariables(scopeOf(get(), id));
    } catch (e) {
      console.error('[store] dynamic_variables_delete failed', e);
      set({ dynamicError: errorText(e) });
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
        // 继承取数环境的定义按这个环境取数；空 = 没有环境，后端回落到项目默认环境。
        environmentId: callerEnvironmentId(get()),
        force: force ?? false,
      });
      // 值写进 store：请求编辑器下一次 `prepareRequest` 会用上它们。
      //
      // 桶按项目分：跨项目取值（不点名项目）时那些值没有项目可落，故归到空串那一格——那一格不
      // 参与任何项目的请求替换。
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
      set({ dynamicError: errorText(e) });
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
        // 覆盖值写在 (定义 × 生效环境) 那一行：继承的定义换个环境就是另一格。
        environmentId: callerEnvironmentId(get()),
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
      set({ dynamicError: errorText(e) });
    }
  },

  refreshDynamicVariable: async (id) => {
    set({ dynamicResolving: true });
    try {
      const status = await invokeCmd<DynamicValueStatus>(COMMANDS.dynamicVariablesRefresh, {
        id,
        // 「刷新」重取的正是当前环境下的那一格。
        environmentId: callerEnvironmentId(get()),
      });
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
      set({ dynamicError: errorText(e) });
    } finally {
      set({ dynamicResolving: false });
    }
  },
});

/**
 * 调用方此刻选中的环境，送给后端当作「继承」的那个值。
 *
 * **不在这里按项目挑**：一个环境算不算数，取决于**每条定义自己的数据源接口**在哪个项目
 * ——那个答案在后端手里（它逐条判定，不属于就回落到项目默认环境）。这里只回答「用户此刻
 * 选的是哪个环境」，多一道前端过滤只会把「接口在别的项目里」的那些定义一起挡掉。
 */
function callerEnvironmentId(state: AppState): string | null {
  return state.activeEnvironmentId || null;
}

/**
 * 一条定义属于哪个项目 —— 也就是「重载列表时该列哪个项目」。
 *
 * 优先用定义自己的归属项目；定义不在已加载的列表里（于是无从查它的归属）时退回当前活动项目
 * ——那是用户正在看的那个，也是面板与请求页要显示的那一个。
 */
function scopeOf(state: AppState, id: string): string | undefined {
  const definition = state.dynamicVariables.find((v) => v.id === id);
  return definition?.projectId ?? (state.activeProjectId || undefined);
}
