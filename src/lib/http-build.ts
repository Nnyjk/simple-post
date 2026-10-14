/**
 * 「我们将要发出什么」的唯一构造核：真实发送（`src/lib/http.ts`）与
 * copy-as-cURL 预览都从这里派生，两边因此不可能对同一请求有不同描述。
 *
 *   prepareRequest()   → 唯一的组装处：请求 → URL + 有序 header + body
 *                        （变量未解析），别处不再拼装请求
 *   buildFetchInit()   → prepareRequest 的 fetch 适配器
 *   buildCurlCommand() → prepareRequest 的 shell 行适配器
 *
 * 纯函数，无 I/O。I/O 归运行时的原生 `fetch`，本模块只把用户可编辑的
 * 请求形状转成运行时要的 URL + init（或 cURL 字符串）。
 */
import type {
  AuthConfig,
  BodyMode,
  Environment,
  HttpMethod,
  KeyValue,
  RequestBody,
} from '@/types/domain';
import { collapseUrlSlashes, joinBaseUrl, resolveVarsWithMissing } from './url';

/**
 * 影响请求**线上形态**的入参。设置项（超时 / 重定向 / 体积上限）与调用方的
 * `AbortSignal` 有意不放这里——它们只在执行期起作用。
 *
 * `url` 是**完整** URL：这里没有 baseUrl 槽位，调用方必须先拼好。
 * 手上正好是 baseUrl 与 path 时用 [`prepareRequest`]；此形状留给既有
 * fetch/cURL 适配器，它们视其为 `baseUrl: ''`。
 */
export interface BuildInput {
  method: HttpMethod;
  url: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
}

export interface BuiltFetch {
  /**
   * 最终 URL：query 参数与 query 位置的 apikey 已追加。
   */
  url: string;
  /** 可直接交给 `fetch` 的 `RequestInit`；`signal` 与 `redirect` 不在其中，由调用方叠加。 */
  init: RequestInit;
  /**
   * body 的字符串回显（form 模式已 URL/form 安全）。留给将来的「看看我们
   * 会发出什么」预览面，`sendRequest` 目前不用它。
   */
  bodyForLog: string;
}

// ---------------------------------------------------------------------------
// prepareRequest — 唯一的构造核
// ---------------------------------------------------------------------------

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
}

export interface PreparedRequest {
  method: HttpMethod;
  /**
   * 完整 URL：`join(baseUrl, urlPath)` → 解析 `{{var}}` → 折叠 path 斜杠 →
   * 解析并追加参数值 → 追加 query 位置的 apikey，顺序如列。
   * 先 join 再解析，环境自身的 base URL 里的变量才解析得到；折叠放在解析
   * 之后，因为把变量替换成以 `/` 开头的值才会把 join 的分隔符变成 `//`；
   * 参数放最后，因为它们的值在进来时已解析（见 `appendQueryString`）。
   */
  url: string;
  /**
   * 有序 header 元组。用列表而非 `Record`：HTTP 允许重名 header，
   * `Record` 会静默把它们合并，而线上形态要能传到后端（那里收
   * `Vec<(String, String)>`）。
   */
  headers: Array<[string, string]>;
  /**
   * body 内容；`mode === 'none'` 或用户填的内容为空时为 `null`。
   * 与其他字段一样模板化：`{{var}}` 对当前环境解析。
   *
   * 判空跑在**用户写的原文**上、模板化之前，与 Rust 的 `resolve_endpoint`
   * 同序；所以内容只有一个变量且解析为 `""` 的 body 照样发空串而不被丢掉。
   */
  body: string | null;
  /**
   * URL / 参数值 / header / body / auth 里全部未解析的 `{{name}}`，去重且按
   * 首次出现排序。集合与 Rust `resolve_endpoint` 的 `missing_vars` 对齐；
   * *顺序*是渲染器本地的，只有本应用的告警提示消费它。
   */
  missingVars: string[];
}

// ---- 内部辅助 ----

/**
 * 跨请求涉及的每个字段累积未解析的 `{{name}}`，按首次出现去重。
 * `Set` 管去重 + 数组管顺序：只用 `Set` 集合相同但顺序依赖插入内部实现，
 * 而顺序正是「缺哪些变量」提示在用户打字时保持稳定的原因。
 */
interface MissingTracker {
  add(names: readonly string[]): void;
  list(): string[];
}

function createMissingTracker(): MissingTracker {
  const seen = new Set<string>();
  const names: string[] = [];
  return {
    add(chunk) {
      for (const n of chunk) {
        if (seen.has(n)) continue;
        seen.add(n);
        names.push(n);
      }
    },
    // 拷贝：`PreparedRequest` 是交给调用方的值对象，不能把仍在追加的数组交出去。
    list: () => names.slice(),
  };
}

/**
 * 把启用的非空参数拼成 `?k=v&...` 查询串。
 *
 * 参数值与其他字段一样是模板：`{{userId}}` 在编码前对当前环境解析。
 * 与 Rust 的 `resolve_endpoint` 一致，只替换**值**、`key` 原样透传——后端不
 * 解析 key 里的变量，这里若解析，cURL 预览与真实发送就会不一致，而那正是
 * 本模块要消除的分歧。
 *
 * 未启用的参数在替换前就跳过，理由与 header 相同：它不会发出，其中的变量
 * 不算这次请求的问题，报出来是误报。
 */
function appendQueryString(
  url: string,
  params: readonly KeyValue[],
  env: Environment | undefined,
  missing: MissingTracker,
): string {
  const parts: string[] = [];
  for (const p of params) {
    if (!p.enabled) continue;
    const value = resolveVarsWithMissing(p.value, env);
    missing.add(value.missing);
    // 空值判断跑在*解析后*的值上：解析为 "" 的变量与手输的空值同样丢掉该参数
    if (!p.key || value.value === '') continue; // spec: skip empty keys/values
    parts.push(`${encodeURIComponent(p.key)}=${encodeURIComponent(value.value)}`);
  }
  if (parts.length === 0) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${parts.join('&')}`;
}

/**
 * 解析 auth config 里的 `{{var}}` 并记录未解析项。返回新对象、不改传入值——
 * auth config 存在 Zustand store 里，发送不能写它。
 *
 * 只扫选中的 `kind` 且只扫会上线的字段：Basic config 上残留的 `bearer`
 * 串不属于这次请求，报它的变量纯属噪声。
 */
function resolveAuth(
  auth: AuthConfig,
  env: Environment | undefined,
  missing: MissingTracker,
): AuthConfig {
  if (auth.kind === 'bearer') {
    const r = resolveVarsWithMissing(auth.bearer ?? '', env);
    missing.add(r.missing);
    return { ...auth, bearer: r.value };
  }
  if (auth.kind === 'basic' && auth.basic) {
    const u = resolveVarsWithMissing(auth.basic.username, env);
    const p = resolveVarsWithMissing(auth.basic.password, env);
    missing.add(u.missing);
    missing.add(p.missing);
    return {
      ...auth,
      basic: { username: u.value, password: p.value },
    };
  }
  if (auth.kind === 'apikey' && auth.apikey) {
    const k = resolveVarsWithMissing(auth.apikey.key, env);
    const v = resolveVarsWithMissing(auth.apikey.value, env);
    missing.add(k.missing);
    missing.add(v.missing);
    return {
      ...auth,
      apikey: { ...auth.apikey, key: k.value, value: v.value },
    };
  }
  return auth;
}

/** 按 body mode 自动补 Content-Type（`none` 返回 `null`）。 */
function autoContentType(mode: BodyMode): string | null {
  switch (mode) {
    case 'json':
      return 'application/json';
    case 'form':
      return 'application/x-www-form-urlencoded';
    case 'raw':
      return 'text/plain';
    default:
      return null;
  }
}

/**
 * 有序 header 列表：先是用户 header，再是自动 Content-Type（缺失时）与
 * Accept，最后是 auth 派生的 header（`Authorization` 或 header 位置的
 * apikey）。查重大小写不敏感——HTTP header 名本就大小写不敏感。
 *
 * `apikey.inLocation === 'query'` 不出 header，调用方通过
 * `appendApikeyToQuery` 拼到 URL 上。
 */
function buildOrderedHeaders(
  headers: readonly KeyValue[],
  bodyMode: BodyMode,
  auth: AuthConfig,
  env: Environment | undefined,
  missing: MissingTracker,
): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const h of headers) {
    if (!h.enabled || !h.key) continue;
    const r = resolveVarsWithMissing(h.value, env);
    missing.add(r.missing);
    out.push([h.key, r.value]);
  }

  // 按 body.mode 自动补 Content-Type。用户已设过（不分大小写）就不加；
  // 只要 mode 蕴含 body 就补，内容空不空由 `init.body` 那一步单独管
  const ct = autoContentType(bodyMode);
  if (ct && !out.some(([k]) => k.toLowerCase() === 'content-type')) {
    out.push(['Content-Type', ct]);
  }

  // 默认 Accept，让服务端知道我们要干净地解析 JSON
  if (!out.some(([k]) => k.toLowerCase() === 'accept')) {
    out.push(['Accept', 'application/json']);
  }

  // auth 派生 header。用户已有同名（不分大小写）时跳过：fetch 用 Record
  // 只会留最后一个值，而 cURL 里两条 `-H 'Authorization: …'` 服务端看到的是
  // header 列表而非覆盖。
  if (auth.kind === 'bearer' && auth.bearer) {
    if (!out.some(([k]) => k.toLowerCase() === 'authorization')) {
      out.push(['Authorization', `Bearer ${auth.bearer}`]);
    }
  } else if (auth.kind === 'basic' && auth.basic) {
    if (!out.some(([k]) => k.toLowerCase() === 'authorization')) {
      const { username, password } = auth.basic;
      out.push(['Authorization', `Basic ${b64Basic(username, password)}`]);
    }
  } else if (auth.kind === 'apikey' && auth.apikey && auth.apikey.inLocation === 'header') {
    if (!out.some(([k]) => k.toLowerCase() === auth.apikey!.key.toLowerCase())) {
      out.push([auth.apikey.key, auth.apikey.value]);
    }
  }

  return out;
}

/**
 * 为 HTTP Basic 认证把 `user:pass` 做 base64。走 `TextEncoder` 是为了先把
 * 含非 ASCII 的凭据（中文用户名、emoji 密码）按 UTF-8 编码成字节再 base64——
 * 浏览器的事实惯例，Express 等多数服务端按裸字节解码这个 header。
 *
 * `btoa` 由 DOM lib 提供，已在项目 tsconfig 的
 * `"lib": ["ES2022", "DOM", "DOM.Iterable"]` 里。
 */
function b64Basic(user: string, pass: string): string {
  const raw = `${user}:${pass}`;
  const bytes = new TextEncoder().encode(raw);
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]!);
  return btoa(bin);
}

/** 把 `inLocation` 为 `'query'` 的 `apikey` 追加到 URL。 */
function appendApikeyToQuery(url: string, auth: AuthConfig): string {
  if (auth.kind !== 'apikey' || !auth.apikey || auth.apikey.inLocation !== 'query') {
    return url;
  }
  const { key, value } = auth.apikey;
  if (!key) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${encodeURIComponent(key)}=${encodeURIComponent(value)}`;
}

/** POSIX shell 单引号转义：`it's` → `'it'\''s'`。 */
function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

/** 仅当 mode 允许且内容非空时才附上 body。 */
function hasBody(body: RequestBody): boolean {
  return body.mode !== 'none' && body.content.length > 0;
}

// ---- 公开 API ----

/**
 * 把用户可编辑的请求形状变成传输层需要的一切。全应用唯一组装请求的函数。
 *
 * 操作顺序（每一步的顺序都有讲究）：
 *
 *   1. join `baseUrl` + `urlPath`——自带 scheme 的 path 优先，「自定义」的
 *      完整 URL 因此不会被前缀；
 *   2. 在 join 后的 URL 上解析 `{{var}}`，环境自身 base URL 里的变量才解析得到；
 *   3. 逐个解析启用参数的**值**并把存活者拼成查询串（编码，并按当前 URL
 *      选 `?` 还是 `&`）；
 *   4. 追加 query 位置的 apikey；
 *   5. 构造有序 header 列表；
 *   6. 决定 body 是否包含（`mode !== 'none'` **且**原文非空），再模板化 body。
 *
 * 每个写下的 `{{var}}`，无论落在 URL、参数值、header、auth 还是 body，解析
 * 不到就记录；每个上线的字段都被模板化，body 也不例外。
 */
export function prepareRequest(input: PrepareRequestInput): PreparedRequest {
  const missing = createMissingTracker();

  const auth = resolveAuth(input.auth, input.env, missing);

  const joined = joinBaseUrl(input.baseUrl, input.urlPath);
  const url = resolveVarsWithMissing(joined, input.env);
  missing.add(url.missing);
  // 折叠放在替换之后、绝不之前：`{{var}}` → `/api` 才会把 `joinBaseUrl`
  // 插入的分隔符变成 `//`，到这里真实 path 才确定。`collapseUrlSlashes` 不碰
  // query/fragment，所以参数还能在它之后追加
  const normalized = collapseUrlSlashes(url.value);

  const withParams = appendQueryString(normalized, input.params, input.env, missing);
  const finalUrl = appendApikeyToQuery(withParams, auth);

  const headers = buildOrderedHeaders(input.headers, input.body.mode, auth, input.env, missing);

  // 先按原文判断是否包含，再模板化——与 Rust 的 `resolve_endpoint` 同序。
  // 先模板化再判断会静默丢掉 body 只是一个解析为空串的变量的请求
  const body = hasBody(input.body) ? resolveVarsWithMissing(input.body.content, input.env) : null;
  if (body !== null) missing.add(body.missing);

  return {
    method: input.method,
    url: finalUrl,
    headers,
    body: body === null ? null : body.value,
    missingVars: missing.list(),
  };
}

/** `BuildInput` → `PrepareRequestInput`，base URL 为空串。
 *
 *  `BuildInput.url` 一向是完整 URL，所以下面的兼容适配器把它当作无 base 可
 *  join 的 path；`joinBaseUrl` 在该输入上是恒等的（空 base 返回 path，自带
 *  scheme 的 path 原样返回）。
 */
function fromBuildInput(input: BuildInput): PrepareRequestInput {
  return {
    method: input.method,
    urlPath: input.url,
    baseUrl: '',
    params: input.params,
    headers: input.headers,
    body: input.body,
    auth: input.auth,
    env: input.env,
  };
}

/**
 * 构造 `fetch(url, init)` 参数对。纯函数，无 I/O、无状态；`signal` 与
 * `redirect` 由调用方叠加。
 *
 * [`prepareRequest`] 之上的薄适配器。header 在这里收回 `Record`（`fetch`
 * 的自然形状），代价是重名 header 只留最后一个值——该入口的既有局限，
 * 也是保留 tuple 形态的 `sendPreparedRequest` 走后台传输的原因。
 */
export function buildFetchInit(input: BuildInput): BuiltFetch {
  const prepared = prepareRequest(fromBuildInput(input));

  const headersRecord: Record<string, string> = {};
  for (const [k, v] of prepared.headers) headersRecord[k] = v;

  const init: RequestInit = {
    method: input.method,
    headers: headersRecord,
  };
  if (prepared.body !== null) {
    init.body = prepared.body;
  }

  return {
    url: prepared.url,
    init,
    bodyForLog: prepared.body ?? '',
  };
}

/**
 * 把已 prepare 的请求渲染成可粘贴的 cURL 命令。
 *
 * **全应用唯一拼 cURL 字符串的地方。** `buildCurlCommand` 只是它在
 * `BuildInput` 之上的适配器：预览若自己重新推导 URL、header 与 body，就是
 * 在描述一个没人发过的请求。
 *
 * 这里读到的一切都已是终态：`url` 已 join 且已解析 `{{var}}`，`headers`
 * 是含 auth 派生 `Authorization` 的有序线上列表，`body` 是真正会写进 socket
 * 的那个字符串。
 */
export function preparedToCurl(prepared: PreparedRequest): string {
  const lines: string[] = [`curl -X ${prepared.method.toUpperCase()} ${shellQuote(prepared.url)}`];
  // 用列表而非 record：`prepared.headers` 保留重名，线上合法，且每行一个 `-H`
  // 正好复现
  for (const [k, v] of prepared.headers) {
    lines.push(`  -H ${shellQuote(`${k}: ${v}`)}`);
  }
  if (prepared.body !== null) {
    lines.push(`  --data-raw ${shellQuote(prepared.body)}`);
  }
  return lines.join(' \\\n');
}

/**
 * 为同一请求生成可粘贴的 cURL 命令。[`prepareRequest`] → [`preparedToCurl`]
 * 之上的薄适配器，所以发出的 URL、header、body 按构造就是真实发送会用的那套。
 */
export function buildCurlCommand(input: BuildInput): string {
  return preparedToCurl(prepareRequest(fromBuildInput(input)));
}
