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
 * 作用域是**项目**：一条定义挂在一个项目里，变量名在项目内唯一。不同项目可以有同名变量而互不
 * 覆盖——基线项目与各个客户分发项目各自取自己的那份数据，正因为它们的名字一样、取值不同。
 * 数据源二选一：`sourceEndpointId` 引用树里的一个端点，`sourceInlineJson` 是一份内联请求。
 */
export interface DynamicVariable {
  id: string;
  /** 归属项目。定义挂在项目里，故不同项目可以有同名变量而互不覆盖。 */
  projectId?: string | null;
  name: string;
  description?: string | null;
  /** 数据源端点（`nodes.id`）。端点被删后这一项为 null，定义本身仍在。 */
  sourceEndpointId?: string | null;
  /** 数据源端点的名字。界面直接显示它，不必自己再查一次树。 */
  sourceEndpointName?: string | null;
  /** 内联请求（method / url / headers / body / auth 的 JSON 文本）。 */
  sourceInlineJson?: string | null;
  /**
   * 取数用的环境。空 = **继承**：用调用方（发送请求时你选中的）那个环境；调用方没有可用
   * 环境时回落到数据源端点所属项目最早建的那个，再没有就是没有环境。
   */
  sourceEnvironmentId?: string | null;
  /** JSONPath 表达式。`$` 表示整段响应。 */
  extractPath: string;
  /**
   * 处理脚本。抽取之后跑，用来做 JSONPath 表达不了的加工（取值、拼接、算账）。
   *
   * 脚本拿得到抽取结果（`pm.data`）与原始响应（`pm.response`），用 `return` 给出最终值。空 =
   * 不跑脚本，抽取结果就是最终值。
   */
  transformScript?: string | null;
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
  | 'too_large'
  /** 跨项目取值时同一个名字出现在多个项目里，无法判断该用哪一条。 */
  | 'ambiguous'
  /** 处理脚本抛错、超时或没有 `return` 出值。 */
  | 'script_error';

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
  /**
   * 实际取数用的环境。定义点名了就那个；没点名就是**调用方此刻选中的环境**（界面里就是
   * 发送请求时你选中的那个），调用方没有可用环境时回落到数据源项目最早建的那个环境。
   */
  effectiveEnvironmentId?: string | null;
  effectiveEnvironmentName?: string | null;
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

/**
 * 预览里的一条问题。
 *
 * `kind` 的取值分两段，消费者按两段穷举才算全覆盖：
 *
 *   - `DynamicErrorKind`：与取数共用的原因，含 `'too_large'`（抽取结果序列化后超过上限）。
 *   - `'http_status'`：响应不是 2xx，`message` 里带数字状态码。
 *   - `'binary'`：正文是二进制，做不了 JSONPath。
 *
 * 后两类只在预览这条路上出现，且都不给 `value`——正文与响应树照给。
 */
export interface DynamicPreviewIssue {
  kind: DynamicErrorKind | 'http_status' | 'binary';
  message: string;
}

/**
 * 一次「用草稿取数」的预览结果。
 *
 * 正文有三种形态，界面必须说清是哪一种：JSON（`bodyJson` 有值，树可以点）、非 JSON 的
 * 文本（`bodyJson` 缺席，`bodyText` 是原文）、二进制（`binary` 为真，此时不能做 JSONPath）。
 *
 * 抽取与处理脚本的失败**不整体失败**：`extractError` / `transformError` 各记一条，正文与
 * 抽取结果照给 —— 用户要照着树重新点一条路径。
 */
export interface DynamicPreviewResult {
  status: number;
  contentType: string;
  durationMs: number;
  /** 正文被全局上限裁过。树是不完整的，界面要明说。 */
  truncated: boolean;
  /** 正文是二进制（图片、压缩包……）。此时不能做 JSONPath，也不给树。 */
  binary: boolean;
  /** 文本正文。二进制响应时它是宽松解码的结果（非法字节已变成 U+FFFD）。 */
  bodyText: string;
  /** 严格解析出来的 JSON。不是 JSON 就缺席。 */
  bodyJson?: unknown;
  /** 实际生效的抽取规则（空串已被换成 `$`）。 */
  extractPath: string;
  /** 实际生效的取数环境。缺席 = 没有环境。 */
  environmentId?: string | null;
  extracted?: unknown;
  extractError?: DynamicPreviewIssue | null;
  /**
   * 预览里的「最终值」：脚本成功用脚本的返回值，否则用抽取结果。
   *
   * 这是**预览产物**，不是会进变量池的运行时值。抽取成功但处理脚本抛错时，预览保留抽取值
   * 并把失败放在 `transformError` 里（好让用户看着树把脚本改对）；运行时把这种失败当成
   * 变量缺失，不拿抽取值顶上。
   */
  value?: unknown;
  transformError?: DynamicPreviewIssue | null;
}
