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
 *   - `dynamicValues`：**(项目 × 取数环境) → 变量名 → 当前值**。取数用的环境由定义决定：
 *     点名的那个，或（定义没点名时）**调用方此刻选中的环境**——也就是顶栏那个
 *     `activeEnvironmentId`。桶按这两维分（键见 `dynamicValueScope`）：在 A 环境取到的值留在
 *     A 那一格，切到 B 环境后界面读的是 B 那一格，不会拿 A 的值去替换；A 那次请求晚回来也只
 *     落进 A 那一格。
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
import { dynamicTabId } from './store-types';

/**
 * 取值桶的键：项目 × 取数环境。
 *
 * 取数环境决定值本身（同一个名字在两个环境下可以是两个值），故它必须进键。两段之间用
 * `\u0000` 分隔：项目 id 与 环境 id 都是后端铸的 uuid 或空串，不会是它，键因此不会歧义。
 */
export function dynamicValueScope(projectId: string, environmentId: string | null): string {
  return `${projectId}\u0000${environmentId ?? ''}`;
}

/**
 * 把一个键拆回项目 id。合并（改名、删除）时要跨这个项目的所有环境格找同名变量。
 */
function scopeProjectId(scope: string): string {
  const cut = scope.indexOf('\u0000');
  return cut < 0 ? scope : scope.slice(0, cut);
}

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
  /**
   * 打开表单那一刻这条定义的样子：编辑时差量比对以它为基线（见 `features/dynamic-variables/edit-patch.ts`）。
   *
   * 不用「此刻的列表项」当基线：列表在表单开着的期间会被刷新（切环境、别处保存都会重载），
   * 那会把别人刚存的改动判成「我改的」再写回去。新建时为 `null`。
   */
  openedWith: DynamicVariable | null;
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

/** 表单逐格写的那部分。`creating` / `editingId` / `openedWith` 由面板的两个按钮决定，表单不碰。 */
export type DynamicVariableDraftFields = Omit<
  DynamicVariableDraft,
  'creating' | 'editingId' | 'openedWith'
>;

/** 一份空白的新建草稿。 */
export function emptyDynamicVariableDraft(): DynamicVariableDraft {
  return {
    creating: true,
    editingId: null,
    openedWith: null,
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
    openedWith: variable,
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
  /** (项目 × 取数环境) → 变量名 → 当前值。键见 `dynamicValueScope`。 */
  dynamicValues: Record<string, Record<string, DynamicValueStatus>>;
  /** (项目 × 取数环境) → 变量名 → 最近一次取数失败的原因。取数成功时该条目被清掉。 */
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

export const createDynamicSlice: StateCreator<AppState, [], [], DynamicSlice> = (set, get) => {
  /**
   * 列表请求的世代号：只有最后发出的那一次的结果才上屏。
   *
   * 切项目或切环境时上一个请求还在路上，它带回来的是**上一个环境**下的当前值；照单全收
   * 就是把别的环境的值显示成这个环境的。
   */
  let listSeq = 0;
  /**
   * 取值请求的世代号，按桶（项目 × 环境）分开记。
   *
   * 同一格连点两次「刷新」时，先发的可能后回来；只有该格最后发出的那一次算数。不同格
   * 之间互不影响，故不能用一个全局号。
   */
  const valueSeq = new Map<string, number>();

  /**
   * 草稿在不在，就决定这个项目的动态变量标签脏不脏。
   *
   * 标签上的草稿是**唯一**一份未保存内容（表单自己不留 state），故「有草稿」与「有未保存
   * 改动」是同一件事——和 endpoint 草稿一创建就被标脏是同一条约定。`closeSavedTabs` 靠这个
   * 集合跳过它，否则关「已保存」标签时会把用户填了一半的表单一起带走。
   */
  const dirtyForDraft = (projectId: string, present: boolean): Set<string> | null => {
    const tabId = dynamicTabId(projectId);
    const dirty = get().dirtyTabIds;
    if (dirty.has(tabId) === present) return null;
    const next = new Set(dirty);
    if (present) next.add(tabId);
    else next.delete(tabId);
    return next;
  };

  return {
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
          dirtyTabIds: dirtyForDraft(projectId, true) ?? s.dirtyTabIds,
        };
      }),

    beginEditDynamicVariableDraft: (variable) =>
      set((s) => {
        const projectId = variable.projectId ?? '';
        // 正在编辑的就是这一条：保留用户改了一半的内容。
        if (s.dynamicDrafts[projectId]?.editingId === variable.id) return {};
        return {
          dynamicDrafts: { ...s.dynamicDrafts, [projectId]: draftFromDefinition(variable) },
          dirtyTabIds: dirtyForDraft(projectId, true) ?? s.dirtyTabIds,
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
        return {
          dynamicDrafts: { ...s.dynamicDrafts, [projectId]: { ...draft, ...patch } },
          dirtyTabIds: dirtyForDraft(projectId, true) ?? s.dirtyTabIds,
        };
      }),

    clearDynamicVariableDraft: (projectId) =>
      set((s) => {
        if (!(projectId in s.dynamicDrafts)) return {};
        const dynamicDrafts = { ...s.dynamicDrafts };
        delete dynamicDrafts[projectId];
        // 草稿没了，这个标签也就不再有未保存内容：「关闭已保存」可以把它一起关掉。
        return { dynamicDrafts, dirtyTabIds: dirtyForDraft(projectId, false) ?? s.dirtyTabIds };
      }),

    /**
     * 把一个变量名从取值桶里剔除。停用或删除之后，它残留的旧值不该留着：请求编辑器把桶里的值
     * 并进变量池，留着就等于停用之后仍然在替换。
     *
     * 桶按项目 × 环境分，故要把这个项目的**每一格**都扫一遍：改名或删除的时候，用户不一定
     * 站在当初取数的那一格上，只清当前环境会在别的环境里留下旧名字。同名变量在别的项目里
     * 还活着，那一份不能被一起删掉。
     */
    dropDynamicValue: (projectId, name) =>
      set((s) => {
        const values = { ...s.dynamicValues };
        const errors = { ...s.dynamicErrors };
        const dropFrom = (buckets: Record<string, Record<string, unknown>>) => {
          for (const [scope, bucket] of Object.entries(buckets)) {
            if (scopeProjectId(scope) !== projectId || !(name in bucket)) continue;
            const next = { ...bucket };
            delete next[name];
            if (Object.keys(next).length === 0) delete buckets[scope];
            else buckets[scope] = next;
          }
        };
        dropFrom(values);
        dropFrom(errors);
        return { dynamicValues: values, dynamicErrors: errors };
      }),

    loadDynamicVariables: async (projectId) => {
      // 号在**发请求之前**取：两次调用各持一个号，回来时只有最后一次的结果上屏。
      const seq = ++listSeq;
      try {
        const views = await invokeCmd<DynamicVariableView[]>(COMMANDS.dynamicVariablesList, {
          // 空串与缺席同义：不点名项目就是跨项目那一路（列出全部）。
          projectId: projectId || null,
          // 列表带回的当前值是「在某个环境下的值」，故要看哪一格缓存得说清是哪个环境。
          environmentId: callerEnvironmentId(get()),
        });
        // 期间又发过一次列表请求：这一份对应的是旧项目或旧环境下的当前值，丢掉。
        if (seq !== listSeq) return;
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
      // 环境在**发请求之前**定下来：这一份结果是在它下面取到的，回来时也落回它那一格。
      const environmentId = callerEnvironmentId(get());
      // 键也是发起时算的：期间用户切了项目或环境，这份结果照样只落在自己的格子里。
      const scope = dynamicValueScope(projectId ?? '', environmentId);
      const seq = (valueSeq.get(scope) ?? 0) + 1;
      valueSeq.set(scope, seq);
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
          environmentId,
          force: force ?? false,
        });
        // 值写进 store：请求编辑器下一次 `prepareRequest` 会用上它们。
        //
        // 同一格（项目 × 环境）连点两次时，先发的那次可能后回来：只有最后一次算数，更早的
        // 结果丢掉，否则会把刚取到的新值盖成旧的。
        if (valueSeq.get(scope) !== seq) return { values: {}, errors: [] };
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
      // 环境在发请求之前定下来：写进去的那一行属于它，回来清失败原因也该清这一格。
      const environmentId = callerEnvironmentId(get());
      try {
        await invokeCmd(COMMANDS.dynamicVariablesSetOverride, {
          id,
          value,
          expiresAt: options?.expiresAt ?? null,
          forever: options?.forever ?? false,
          // 覆盖值写在 (定义 × 生效环境) 那一行：继承的定义换个环境就是另一格。
          environmentId,
        });
        // 覆盖值在手就不用再取数，上一次取数失败的原因随之作废。
        const definition = get().dynamicVariables.find((v) => v.id === id);
        const name = definition?.name;
        const scope = dynamicValueScope(definition?.projectId ?? '', environmentId);
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
      // 「刷新」重取的正是当前环境下的那一格：环境与格子键都在发请求之前定下来。
      const environmentId = callerEnvironmentId(get());
      const projectId = get().dynamicVariables.find((v) => v.id === id)?.projectId ?? '';
      const scope = dynamicValueScope(projectId, environmentId);
      const seq = (valueSeq.get(scope) ?? 0) + 1;
      valueSeq.set(scope, seq);
      set({ dynamicResolving: true });
      try {
        const status = await invokeCmd<DynamicValueStatus>(COMMANDS.dynamicVariablesRefresh, {
          id,
          environmentId,
        });
        // 同一格连点两次「刷新」时，先发的可能后回来：只有最后一次算数。
        if (valueSeq.get(scope) !== seq) return;
        // 这次强制重取的结果（值与失败原因）写进 store，面板立刻能显示。
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
  };
};

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
