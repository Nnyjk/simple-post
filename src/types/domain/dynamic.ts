import type { ResourceNode } from './tree';

/**
 * 值的序列化形态。与 Rust 的 `domain::ValueMode` 一一对应。
 *
 * `auto` 与 `text` 的输出完全一致：对象与数组给裸 JSON 片段，标量给纯文本。
 * `text` 是用户的声明（值会被放进请求体的字符串位置，转义由用户承担）。
 */
export type ValueMode = 'auto' | 'json' | 'text';

/**
 * 一条动态变量的定义：变量名 → 数据源 → 抽取规则 → 缓存时长。
 *
 * 作用域是全局的：定义不属于任何项目，任何项目的端点都能引用它，整个工作区里
 * 变量名唯一。数据源二选一：`sourceEndpointId` 引用树里的一个端点，
 * `sourceInlineJson` 是一份内联请求。
 */
export interface DynamicVariable {
  id: string;
  name: string;
  description?: string | null;
  /** 数据源端点（`nodes.id`）。端点被删后这一项为 null，定义本身仍在。 */
  sourceEndpointId?: string | null;
  /** 数据源端点的名字。界面直接显示它，不必自己再查一次树。 */
  sourceEndpointName?: string | null;
  /** 内联请求（method / url / headers / body / auth 的 JSON 文本）。 */
  sourceInlineJson?: string | null;
  /** 取数用的环境。空 = 数据源端点所属项目的第一个可用环境。 */
  sourceEnvironmentId?: string | null;
  /** 取数用的服务。空 = 数据源接口所属项目的默认服务。 */
  sourceBaseUrlDefinitionId?: string | null;
  /** JSONPath 表达式。`$` 表示整段响应。 */
  extractPath: string;
  valueMode: ValueMode;
  /** 缓存有效期（秒）。 */
  ttlSeconds: number;
  /** 取数失败时是否允许使用已过期的缓存值。 */
  staleFallback: boolean;
  /** 是否参与解析。停用的定义不参与替换。 */
  enabled: boolean;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
}

/** 当前值是从哪来的。 */
export type DynamicValueSource = 'auto' | 'override' | 'stale' | 'missing';

/** 取数或解析失败的原因。 */
export type DynamicErrorKind =
  | 'network'
  | 'timeout'
  | 'status'
  | 'not_json'
  | 'no_match'
  | 'source_missing'
  | 'source_error'
  | 'too_large';

/** 一条动态变量的错误。`name` 是出错的变量名。 */
export interface DynamicError {
  name: string;
  kind: DynamicErrorKind;
  message: string;
}

/** 一条动态变量的当前状态。 */
export interface DynamicValueStatus {
  name: string;
  /** 当前生效的值（已按 `valueMode` 序列化）。 */
  value?: string | null;
  source: DynamicValueSource;
  fetchedAt?: number | null;
  expiresAt?: number | null;
  isOverride: boolean;
  error?: DynamicError | null;
}

/** 一条定义 + 它的当前值与缓存状态。 */
export interface DynamicVariableView extends DynamicVariable {
  value?: string | null;
  source: DynamicValueSource;
  fetchedAt?: number | null;
  expiresAt?: number | null;
  isOverride: boolean;
  error?: DynamicError | null;
  /** 数据源端点所属的项目。内联请求没有项目。 */
  sourceProjectId?: string | null;
  sourceProjectName?: string | null;
  /** 实际取数用的环境：定义点名的那个，或数据源项目里的第一个可用环境。 */
  effectiveEnvironmentId?: string | null;
  effectiveEnvironmentName?: string | null;
  /** 实际取数用的服务：定义点名的那个，或数据源项目的默认服务。 */
  effectiveBaseUrlDefinitionId?: string | null;
  effectiveBaseUrlDefinitionName?: string | null;
}

/** 跨项目搜到的一条接口，带它所在的路径。 */
export interface NodeSearchHit {
  node: ResourceNode;
  /** 从项目到直接父节点的名字，自上而下。 */
  path: string[];
}

/** 一次解析的结果：值、状态与错误。 */
export interface DynamicResolution {
  /** 变量名 → 生效的值。缺席的名字按缺失处理。 */
  values: Record<string, string>;
  statuses: DynamicValueStatus[];
  errors: DynamicError[];
}
