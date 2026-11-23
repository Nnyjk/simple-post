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

/**
 * 这份列表是不是当前这个 (项目 × 取数环境) 拉到的。
 *
 * 切换环境的那一刻列表内容还是上一个环境的：面板此刻若让用户去改「当前值」，改的是 A 环境
 * 那一格的值，保存却会写进 B 环境。故面板拿这个判定要不要让值相关的操作可用。
 */
export function dynamicListMatchesEnvironment(
  listScope: string | null,
  projectId: string,
  environmentId: string | null,
): boolean {
  return listScope !== null && listScope === dynamicValueScope(projectId, environmentId);
}

/**
 * 「这个项目正在新建定义」的保存中标志键。
 *
 * 新建时还没有定义 id 可作键，故按项目记：同一项目的两次新建是同一件事的两个请求，第二次
 * 必须被挡住——否则会落成两条定义，撞项目内名字唯一约束。
 */
export function creatingDraftKey(projectId: string): string {
  return `new:${projectId}`;
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
   * 这会话的号。每个项目各自自增，`beginCreateDynamicVariableDraft` 与
   * `beginEditDynamicVariableDraft` 各推进一次。
   *
   * 收尾要比对的是**归属**而不是内容：存 A 的 PATCH 还在路上时，用户可以取消并另开一张新建
   * 表——新草稿的 `editingId` 也是 `null`，跟「存 A 时那份草稿」长得一样。只比内容就会把 A
   * 的完成回调套到这张新表单上，把它改写成「正在编辑 A」，下一次保存直接覆盖 A。
   */
  draftSession: number;
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
  'creating' | 'editingId' | 'draftSession' | 'openedWith'
>;

/** 一份空白的新建草稿。 */
export function emptyDynamicVariableDraft(): DynamicVariableDraft {
  return {
    creating: true,
    editingId: null,
    draftSession: 0,
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
    draftSession: 0,
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

/**
 * 把「打开表单时的基线」推进到刚保存的那一份，但**只推进这次提交碰过的字段**。
 *
 * 没被这次 patch 提到的字段保持原基线：客户端 A 只改了名字，客户端 B 刚把说明改成 B，服务端
 * 响应里带着 B；整份换成响应，A 的草稿还写着旧说明，下一次保存就把 B 覆盖回去了。
 */
function advanceDraftBaseline(
  openedWith: DynamicVariable,
  saved: DynamicVariable,
  patch: Partial<DynamicVariable>,
): DynamicVariable {
  const next: DynamicVariable = { ...openedWith };
  for (const key of Object.keys(patch) as (keyof DynamicVariable)[]) {
    (next as unknown as Record<string, unknown>)[key] = saved[key];
  }
  return next;
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
  /**
   * `dynamicVariables` 是在哪个 (项目 × 取数环境) 下拉到的（键见 `dynamicValueScope`）；
   * 还没载过时是 `null`。
   *
   * 列表里的「当前值」属于某个环境。切环境的那一刻列表还在路上，内容仍是上一个环境的；
   * 面板据此判断要不要让「写覆盖值」这类依赖值的操作可用（判定见
   * `dynamicListMatchesEnvironment`）。
   */
  dynamicVariablesScope: string | null;
  /** (项目 × 取数环境) → 变量名 → 当前值。键见 `dynamicValueScope`。 */
  dynamicValues: Record<string, Record<string, DynamicValueStatus>>;
  /** (项目 × 取数环境) → 变量名 → 最近一次取数失败的原因。取数成功时该条目被清掉。 */
  dynamicErrors: Record<string, Record<string, DynamicError>>;
  /**
   * 还在路上的值请求条数。界面上据此显示「正在取数…」。
   *
   * 计数而不是布尔：成批解析与单条刷新会同时在路上，先回来的那一条不该把按钮提前恢复成
   * 可点——那一批还没跑完。
   */
  dynamicInFlight: number;
  /**
   * 最近一次列表请求还在路上。面板据此显示加载态：请求失败时列表不会再来，故不能只按
   * 「条数与环境戳」判断是否还在读。
   *
   * 只有最后发出的那一次请求（`seq === listSeq`）能改这个标志，过期请求的成败都不落地。
   */
  dynamicListPending: boolean;
  /**
   * 正在保存的东西（请求在路上）：已存定义用它的 id，新建用 `creatingDraftKey(projectId)`。
   *
   * 放在 store 而不是表单的 `useState` 里：切走标签表单就卸载，`busy` 随之归零，用户切回来
   * 还能再点一次保存——编辑会多发一个 PATCH，新建会落成两条定义。标志在 store 里就跟请求
   * 同寿，切回来仍是「保存中」，按钮仍然禁用。
   */
  savingDynamicVariableIds: Set<string>;
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
   * 丢掉某个项目的草稿。清理点是：点「取消」、被编辑的定义删掉、这个项目的动态变量标签
   * 关掉（`closeTab` / `pruneStaleTabs` / 批量关闭）或项目被删。
   * **切标签、切项目都不清**——那正是把草稿搬出组件树的原因。
   */
  clearDynamicVariableDraft: (projectId: string) => void;
  /**
   * 保存成功后收尾。
   *
   * 当前草稿仍是**提交的那一份**就清掉；保存请求在路上时用户又敲了一格，那一格属于下一份
   * 草稿，于是留着它，但把身份换成刚落地的那一份（`saved`）：不换，再点保存要么以旧基线算
   * patch 而漏发，要么还是 `creating` 状态、再创建一条撞上项目内的名字唯一约束。
   *
   * 归属按 `draftSession` 认，不按内容认：存 A 的请求在路上时取消并另开新建表，新草稿的
   * `editingId` 也是 `null`。不是发起这次保存的那个会话，这里什么都不动。
   *
   * 基线（`openedWith`）只推进 `patch` 碰过的字段：别的客户端刚改了说明，本地这次只存了
   * 名字，整份推进基线会让下次保存把旧的说明发回去，悄悄回滚别人的改动。
   */
  finishDynamicVariableSave: (
    projectId: string,
    submitted: DynamicVariableDraft,
    saved: DynamicVariable,
    patch: Partial<DynamicVariable>,
  ) => void;
  /**
   * 载入定义。
   *
   * 给了 `projectId` 就只载入那个项目的；不给就是跨项目那一路（列出全部）。
   * 只读缓存行，**不取数**。
   */
  loadDynamicVariables: (projectId?: string) => Promise<void>;
  /** 新建一条定义，返回后端落库的那一份（失败返回 null）。 */
  createDynamicVariable: (input: CreateDynamicVariableInput) => Promise<DynamicVariable | null>;
  /**
   * 改一条定义。成功返回后端落库的那一份（失败返回 null，原因在 `dynamicError` 里）。
   *
   * 同一条定义在飞期间再提交会被直接挡回 null：web 传输下两个 PATCH 可以乱序落地，
   * 旧的那次后到就会盖掉新的编辑。挡住的是这一条定义，别的定义照常能存。
   */
  updateDynamicVariable: (
    id: string,
    patch: Partial<DynamicVariable>,
  ) => Promise<DynamicVariable | null>;
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
  /**
   * 写覆盖值。`value` 为 null 表示清除。
   *
   * 返回这次写入是否成功：失败原因写在 `dynamicError` 里，调用方据此决定要不要保留用户的输入。
   */
  setDynamicOverride: (
    id: string,
    value: string | null,
    options?: { expiresAt?: number | null; forever?: boolean },
  ) => Promise<boolean>;
  /** 强制重新取一次数。 */
  refreshDynamicVariable: (id: string) => Promise<void>;
  /**
   * 全部刷新：当前项目的全部启用定义各强制重取一次，然后重载列表。
   *
   * 走的是 `force` —— 与卡片上的单条「刷新」同一条语义。发送时那次解析不强制（TTL 内命中
   * 缓存是它的本分），而「刷新」按钮的职责就是让缓存失效；少了 `force`，按钮按下去值不动。
   *
   * `force` 也跳过手动指定的值，故手写的值会变回自动取回（徽标随之变化，可再写一次）。
   * 批量的行为刻意与逐条刷新一致：同一句话在单条与批量上说两套语义，下一个人必然记错一套。
   */
  refreshAllDynamicVariables: (projectId: string) => Promise<void>;
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
   * 取值请求的世代号，按「格（项目 × 环境）× 变量名」分开记。
   *
   * 同一格连点两次「刷新」时，先发的可能后回来；只有该格最后发出的那一次算数。粒度必须
   * 细到名字：成批解析与单条刷新会同时在路上，按整格记号会让后发的单条刷新挡掉整批里
   * 其它变量的结果，那些变量在变量池里就缺了。
   */
  const valueSeq = new Map<string, number>();

  /** 一格里的一个变量名。`\u0000` 分隔，故格键（自身含 `\u0000`）与名字不会拼出歧义。 */
  const valueKey = (scope: string, name: string) => `${scope}\u0000${name}`;

  /** 给这一对取一个新号并返回：回来时号还是它，这份结果才落进那儿。 */
  const issueValueSeq = (scope: string, name: string): number => {
    const key = valueKey(scope, name);
    const seq = (valueSeq.get(key) ?? 0) + 1;
    valueSeq.set(key, seq);
    return seq;
  };

  /**
   * 让**这个项目里**这个名字在所有格上在飞着的发布作废。
   *
   * 定义被改了或删了（抽取规则、数据源……），按旧定义取到的值不该再落进桶里——否则保存之后
   * 桶里还留着旧规则算出来的值，看上去像新规则的结果。
   *
   * 键是 `项目 \u0000 环境 \u0000 变量名`，故先按前缀认出项目：别的项目里同名变量那一份
   * 还在飞的东西不能被一起作废，那会让它那一格白白丢掉刚取到的值。
   */
  const invalidateName = (projectId: string, name: string) => {
    for (const [key, seq] of valueSeq) {
      if (!key.endsWith(`\u0000${name}`) || scopeProjectId(key) !== projectId) continue;
      valueSeq.set(key, seq + 1);
    }
  };

  /**
   * 每个项目的草稿会话号，`beginCreate…` / `beginEdit…` 各推进一格。
   *
   * 只放在闭包里：它唯一的作用是给「这份草稿归谁」一个能比的号，界面不看它，故不进 state，
   * 也不会被快照回滚搅乱。
   */
  const draftSessions = new Map<string, number>();

  /** 给这个项目开一个新会话并返回它的号。 */
  const issueDraftSession = (projectId: string): number => {
    const session = (draftSessions.get(projectId) ?? 0) + 1;
    draftSessions.set(projectId, session);
    return session;
  };

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

  /**
   * 清掉这个项目的草稿，连带摘掉标签的脏标记。没有草稿时返回 null（调用方据此不 set）。
   *
   * 草稿没了，这个标签也就不再有未保存内容：「关闭已保存」可以把它一起关掉。
   */
  const clearedDrafts = (
    s: AppState,
    projectId: string,
  ): Pick<AppState, 'dynamicDrafts' | 'dirtyTabIds'> | null => {
    if (!(projectId in s.dynamicDrafts)) return null;
    const dynamicDrafts = { ...s.dynamicDrafts };
    delete dynamicDrafts[projectId];
    return { dynamicDrafts, dirtyTabIds: dirtyForDraft(projectId, false) ?? s.dirtyTabIds };
  };

  return {
    dynamicVariables: [],
    dynamicVariablesScope: null,
    dynamicDrafts: {},
    dynamicValues: {},
    dynamicErrors: {},
    dynamicInFlight: 0,
    dynamicListPending: false,
    savingDynamicVariableIds: new Set<string>(),
    dynamicError: null,

    beginCreateDynamicVariableDraft: (projectId) =>
      set((s) => {
        // 已经开着一张新建表：沿用它的内容，只保证它确实开着。
        const draft = s.dynamicDrafts[projectId];
        if (draft?.creating) return {};
        return {
          dynamicDrafts: {
            ...s.dynamicDrafts,
            [projectId]: {
              ...emptyDynamicVariableDraft(),
              draftSession: issueDraftSession(projectId),
            },
          },
          dirtyTabIds: dirtyForDraft(projectId, true) ?? s.dirtyTabIds,
        };
      }),

    beginEditDynamicVariableDraft: (variable) =>
      set((s) => {
        const projectId = variable.projectId ?? '';
        // 正在编辑的就是这一条：保留用户改了一半的内容。
        if (s.dynamicDrafts[projectId]?.editingId === variable.id) return {};
        return {
          dynamicDrafts: {
            ...s.dynamicDrafts,
            [projectId]: {
              ...draftFromDefinition(variable),
              draftSession: issueDraftSession(projectId),
            },
          },
          dirtyTabIds: dirtyForDraft(projectId, true) ?? s.dirtyTabIds,
        };
      }),

    patchDynamicVariableDraft: (projectId, patch) =>
      set((s) => {
        // 浅合并：一个项目一份草稿，改一格就换一个新对象，别的项目的引用原样保留（选择器靠
        // 引用比较，重建整张表会让没在编辑的项目的面板也重渲染一次）。
        //
        // 没有草稿时回退的那份既不是新建表也不是编辑表：回退若带 `creating: true`，一次没人
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

    clearDynamicVariableDraft: (projectId) => set((s) => clearedDrafts(s, projectId) ?? {}),

    finishDynamicVariableSave: (projectId, submitted, saved, patch) =>
      set((s) => {
        const current = s.dynamicDrafts[projectId];
        // 草稿已经被别处清掉了（取消、关标签）：没什么要收的。
        if (current === undefined) return {};
        // 不是发起这次保存的那个会话：用户已经取消并另开了表单（新建表也长得像「没在编辑」），
        // 这份草稿不归这次保存管，连基线都不许推——推了就是把新表单改写成上一条的编辑态。
        if (current.draftSession !== submitted.draftSession) return {};
        // 还是提交的那一份：内容都落了库，草稿可以走。
        if (current === submitted) return clearedDrafts(s, projectId) ?? {};
        // 保存期间又敲了内容：留着这一份，但把它改写成「正在编辑刚落地的那条」。
        // 不改写的话 `creating` 还挂着，再点保存就是第二次创建。
        //
        // 基线只推进这次 patch 碰过的字段：别的客户端刚把说明改成 B，本地这次只存了名字，
        // 整份换成响应会让下一次保存把草稿里的旧说明发回去。
        const openedWith = advanceDraftBaseline(
          submitted.creating ? saved : (current.openedWith ?? saved),
          saved,
          patch,
        );
        return {
          dynamicDrafts: {
            ...s.dynamicDrafts,
            [projectId]: {
              ...current,
              creating: false,
              editingId: saved.id,
              openedWith,
            },
          },
        };
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
      // 环境也在发请求之前取定：结果属于**请求时**那个环境，戳就得记那一个，不是回来时
      // 用户碰巧站着的那个。
      const environmentId = callerEnvironmentId(get());
      const seq = ++listSeq;
      // 请求发出前就翻成「在读」：这一次的结果回来（成功或失败）才翻回去。
      set({ dynamicListPending: true });
      try {
        const views = await invokeCmd<DynamicVariableView[]>(COMMANDS.dynamicVariablesList, {
          // 空串与缺席同义：不点名项目就是跨项目那一路（列出全部）。
          projectId: projectId || null,
          // 列表带回的当前值是「在某个环境下的值」，故要看哪一格缓存得说清是哪个环境。
          environmentId,
        });
        // 期间又发过一次列表请求：这一份对应的是旧项目或旧环境下的当前值，丢掉。
        if (seq !== listSeq) return;
        // 环境戳与结果同一次写入：面板据此判断「这批当前值还是不是这个环境的」。
        set({
          dynamicVariables: views,
          dynamicVariablesScope: dynamicValueScope(projectId ?? '', environmentId),
          dynamicError: null,
          dynamicListPending: false,
        });
      } catch (e) {
        console.error('[store] dynamic_variables_list failed', e);
        // 期间又发过一次：这一份的失败属于旧项目或旧环境，别把它盖到新的那一屏上。
        if (seq !== listSeq) return;
        set({ dynamicError: errorText(e), dynamicListPending: false });
      }
    },

    createDynamicVariable: async (input) => {
      // 同一项目一次只发一个 POST：新建时还没有定义 id 可挡，故按项目记。用户在请求在路上时
      // 切走标签再切回来，表单的 `busy` 已归零，靠这个标志仍然拦住第二次（否则落成两条定义）。
      const savingKey = creatingDraftKey(input.projectId);
      if (get().savingDynamicVariableIds.has(savingKey)) return null;
      set((s) => ({
        savingDynamicVariableIds: new Set(s.savingDynamicVariableIds).add(savingKey),
      }));
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
        // 落库的那一份交回去：提交期间草稿若又变了，表单要把草稿改写成这条的编辑态。
        return created;
      } catch (e) {
        console.error('[store] dynamic_variables_create failed', e);
        set({ dynamicError: errorText(e) });
        return null;
      } finally {
        set((s) => {
          const next = new Set(s.savingDynamicVariableIds);
          next.delete(savingKey);
          return { savingDynamicVariableIds: next };
        });
      }
    },

    updateDynamicVariable: async (id, patch) => {
      // 同一条定义一次只发一个 PATCH：web 传输下两个请求可以乱序落地，旧的那个后到就会
      // 盖掉新编辑。挡住的是这一条定义，别的定义照常能存。
      if (get().savingDynamicVariableIds.has(id)) return null;
      set((s) => ({ savingDynamicVariableIds: new Set(s.savingDynamicVariableIds).add(id) }));
      // 改之前的名字：改名之后旧名下的值与错误都不再有意义。
      const before = get().dynamicVariables.find((v) => v.id === id);
      try {
        const updated = await invokeCmd<DynamicVariable>(COMMANDS.dynamicVariablesUpdate, {
          id,
          patch,
        });
        if (before) {
          // 定义变了：按旧定义取到、还在路上的结果不能再落进桶里。
          invalidateName(before.projectId ?? '', before.name);
          get().dropDynamicValue(before.projectId ?? '', before.name);
        }
        await get().loadDynamicVariables(scopeOf(get(), id));
        return updated;
      } catch (e) {
        console.error('[store] dynamic_variables_update failed', e);
        set({ dynamicError: errorText(e) });
        return null;
      } finally {
        set((s) => {
          const next = new Set(s.savingDynamicVariableIds);
          next.delete(id);
          return { savingDynamicVariableIds: next };
        });
      }
    },

    deleteDynamicVariable: async (id) => {
      const before = get().dynamicVariables.find((v) => v.id === id);
      try {
        await invokeCmd<void>(COMMANDS.dynamicVariablesDelete, { id });
        if (before) {
          // 定义没了：还在路上的取数结果不能把它加回桶里。
          invalidateName(before.projectId ?? '', before.name);
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
      // 每个名字各持一个号：成批解析与单条刷新会同时在路上，谁也别挡谁。
      const issued = new Map<string, number>();
      for (const name of names) issued.set(name, issueValueSeq(scope, name));
      /** 这个名字的结果还没被同名的新请求顶掉。 */
      const fresh = (name: string) => valueSeq.get(valueKey(scope, name)) === issued.get(name);
      // 计数不计数布尔：别的值请求还在路上时，这一次先回来不该把「正在取数」摘掉。
      set((s) => ({ dynamicInFlight: s.dynamicInFlight + 1 }));
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
        // 逐个名字判新鲜度：连点两次时先发的那次可能后回来，被顶掉的条目不能盖掉新值；
        // 但**返回值照给调用方**——它自己那次请求要拿这份结果去替换占位符。
        const applied = resolution.statuses.filter((status) => fresh(status.name));
        const freshErrors = resolution.errors.filter((error) => fresh(error.name));
        set((s) => {
          const bucket = { ...(s.dynamicValues[scope] ?? {}) };
          for (const status of applied) {
            bucket[status.name] = status;
          }
          // 失败原因按变量名记下来，面板要能说清「为什么这个变量没有值」。
          const errors = { ...(s.dynamicErrors[scope] ?? {}) };
          for (const error of freshErrors) {
            errors[error.name] = error;
          }
          for (const status of applied) {
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
        set((s) => ({ dynamicInFlight: s.dynamicInFlight - 1 }));
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
        const projectId = definition?.projectId ?? '';
        const scope = dynamicValueScope(projectId, environmentId);
        if (name) {
          // 手上这份覆盖值盖住的正是这个名字的所有格子：之前发起的刷新还没回来，它带的是
          // 自动取回的值，落进桶里就会把手写的这份顶掉（徽标写着「手动指定」，值却是自动的）。
          invalidateName(projectId, name);
          set((s) => {
            const errors = { ...(s.dynamicErrors[scope] ?? {}) };
            delete errors[name];
            return { dynamicErrors: { ...s.dynamicErrors, [scope]: errors } };
          });
        }
        await get().loadDynamicVariables(scopeOf(get(), id));
        return true;
      } catch (e) {
        console.error('[store] dynamic_variables_set_override failed', e);
        set({ dynamicError: errorText(e) });
        return false;
      }
    },

    refreshAllDynamicVariables: async (projectId) => {
      // 只取这个项目的启用定义：停用的那些后端在按名解析时也会跳过，送上去只是多几次查找。
      const names = get()
        .dynamicVariables.filter((v) => v.projectId === projectId && v.enabled)
        .map((v) => v.name);
      if (names.length === 0) return;
      // `force` 是这条动作的全部意义：不带它，后端在 TTL 内命中缓存，按钮按下去值不动。
      await get().resolveDynamicVariables(names, projectId, true);
      // 值落在取值桶里，而卡片读的是列表：不重载，值、来源徽标与取数时间都不动。
      await get().loadDynamicVariables(projectId);
    },

    refreshDynamicVariable: async (id) => {
      // 「刷新」重取的正是当前环境下的那一格：环境与格子键都在发请求之前定下来。
      const environmentId = callerEnvironmentId(get());
      const definition = get().dynamicVariables.find((v) => v.id === id);
      const projectId = definition?.projectId ?? '';
      const scope = dynamicValueScope(projectId, environmentId);
      // 号只压住这一个名字：同一格里的其它变量（成批解析在路上）不受影响。
      const name = definition?.name ?? '';
      const seq = issueValueSeq(scope, name);
      set((s) => ({ dynamicInFlight: s.dynamicInFlight + 1 }));
      try {
        const status = await invokeCmd<DynamicValueStatus>(COMMANDS.dynamicVariablesRefresh, {
          id,
          environmentId,
        });
        // 同一条定义连点两次「刷新」时，先发的可能后回来：只有最后一次算数。
        if (valueSeq.get(valueKey(scope, name)) !== seq) return;
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
        set((s) => ({ dynamicInFlight: s.dynamicInFlight - 1 }));
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
