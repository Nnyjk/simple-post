import type { AuthConfig, Environment, HttpMethod, KeyValue, RequestBody } from '@/types/domain';

/**
 * 影响请求**线上形态**的入参。超时 / 重定向 / 体积上限与调用方的 `AbortSignal`
 * 只在执行期起作用，不在这里。
 *
 * `url` 是**完整** URL：这里没有 baseUrl 槽位，调用方必须先拼好。手上正好是
 * baseUrl 与 path 时用 `prepareRequest`；此形状留给 fetch 与 cURL 两个适配器，
 * 它们视其为 `baseUrl: ''`。
 */
export interface BuildInput {
  method: HttpMethod;
  url: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
  /**
   * URL 路径里的占位符与它们各自的值。它们替换进路径本身，不进查询串。
   */
  pathParams?: KeyValue[];
}

/** fetch 适配器的产出：URL、`RequestInit` 与 body 回显。 */
export interface BuiltFetch {
  /**
   * 最终 URL：query 参数与 query 位置的 apikey 已追加。
   */
  url: string;
  /** 可直接交给 `fetch` 的 `RequestInit`；`signal` 与 `redirect` 不在其中，由调用方叠加。 */
  init: RequestInit;
  /** body 的字符串回显（form 模式已 URL/form 安全）。`sendRequest` 目前不用它。 */
  bodyForLog: string;
}

/**
 * `prepareRequest` 的入参。`urlPath` 与 `baseUrl` 分开，故「base URL + path」
 * 与「自带 scheme 的完整 URL」两种形态都能表达。
 */
export interface PrepareRequestInput {
  method: HttpMethod;
  /**
   * URL 的 path 部分，不含任何 base URL，可带 `{{var}}`。自带 scheme
   * （`https://…`）的 path 优先于 `baseUrl`——endpoint 的「自定义」模式
   * 靠这点成立：用户手输整个地址，选择器里仍有服务被选中。
   */
  urlPath: string;
  /** 已解析的 base URL；空串表示没有 base URL。 */
  baseUrl: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
  /**
   * URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。
   *
   * 它们替换进路径本身，不像 `params` 那样进查询串。
   */
  pathParams?: KeyValue[];
  /**
   * 动态变量的当前值：变量名 → 替换用的文本。
   *
   * 它们在变量池的**最后一步**并入，故同名时压过环境变量；被停用的环境变量也不影响
   * 它们（停用过滤只作用于环境变量）。合并规则见 `src/lib/dynamic-vars.ts` 的
   * `mergePool`，与 Rust 的 `core::dynamic::merge_pool` 同一套，由
   * `fixtures/dynamic-parity.json` 钉住。
   */
  dynamicVars?: Record<string, string>;
}

/** `prepareRequest` 的产出：这一发请求的全部线上内容。 */
export interface PreparedRequest {
  method: HttpMethod;
  /**
   * 完整 URL：`join(baseUrl, urlPath)` → 替换路径里的 `{占位符}` → 解析 `{{var}}` →
   * 折叠 path 斜杠 → 解析并追加参数值 → 追加 query 位置的 apikey，顺序如列。
   * 先 join 再解析，环境自身的 base URL 里的变量才解析得到；折叠放在解析之后，
   * 因为把变量替换成以 `/` 开头的值才会把 join 的分隔符变成 `//`；参数放最后，
   * 因为它们的值在进来时已解析（见 `appendQueryString`）。
   */
  url: string;
  /**
   * 有序 header 元组。用列表而非 `Record`：HTTP 允许重名 header，`Record` 会
   * 静默把它们合并，而线上形态要能传到后端（那里收 `Vec<(String, String)>`）。
   */
  headers: Array<[string, string]>;
  /**
   * body 内容；`mode === 'none'` 或用户填的内容为空时为 `null`。
   * 与其他字段一样模板化：`{{var}}` 对当前环境解析。
   *
   * 判空跑在**用户写的原文**上、模板化之前，与 Rust 的 `resolve_endpoint` 同序；
   * 所以内容只有一个变量且解析为 `""` 的 body 照样发空串而不被丢掉。
   *
   * `multipart` 下这里已是**装配好的** body（`content` 里的分片数组变成了带
   * boundary 的 multipart 文本），与 `headers` 里那条 Content-Type 同源。
   */
  body: string | null;
  /**
   * URL / 参数值 / header / body / auth 里全部未解析的 `{{name}}`，去重且按
   * 首次出现排序。集合与 Rust `resolve_endpoint` 的 `missing_vars` 对齐；
   * *顺序*是渲染器本地的，只有本应用的告警提示消费它。
   */
  missingVars: string[];
  /**
   * URL 路径里没取到值的占位符名（`/users/{id}` 的 `id`）。
   *
   * 与 `missingVars` 是两回事：那边是「变量池里没有这个变量」，这边是「端点自己的
   * 路径参数没有填值」。发送那侧按这份清单拦下请求——路径参数按 OpenAPI 惯例是必填的，
   * 缺值发出去只会换一个含义不明的 404。
   */
  missingPathParams: string[];
}
