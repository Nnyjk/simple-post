/**
 * 真实 HTTP 发送 —— 一条传输路径、两种分发形态：
 *
 *   1. Tauri shell  → `invokeCmd(COMMANDS.httpRequest, …)` 命中 Rust 的
 *                     `http_request` 命令，它委派给 `http_executor::execute`
 *                     （reqwest）。完全绕开 webview 的 CORS。
 *   2. Web / Dev    → `invokeCmd` 落到 `webInvokeCmd` → axum `serve-web` 上的
 *                     `POST /api/http/send`，调用同一个 `http_executor::execute`。
 *                     同一条 Rust 路径，同一种响应形状。
 *
 * 两种形态都返回同一个 `RustHttpResult` 信封；应用其余部分（HttpTester、
 * ResponseViewer、cURL 导出）不关心跑的是哪条。
 *
 * 契约：
 *   - URL + init 由 `buildFetchInit` 构造（与 cURL 导出共享的唯一真相）。
 *     只发送 URL、header（元组数组）与 body **字符串**，其余是 JSON 友好的载荷。
 *   - 由调用方的 `signal` 组合出每次调用的 AbortController，并按
 *     `settings.timeoutMs` 定时。signal 会传给 `invokeCmd`，web 形态下它交给
 *     原生 `fetch`；Tauri 形态只 reject JS promise，Rust 里的 `reqwest` 仍跑到底。
 *   - 返回 `ApiResponse`。网络失败、超时与中止都以 `status: 0` 加可区分的
 *     `statusText` 上报，UI 据此渲染对应横幅。
 *   - 绝不抛异常——`HttpTester` 只把返回的 `ApiResponse` 塞进 store。
 */
import type {
  ApiResponse,
  AuthConfig,
  Environment,
  EnvSnapshot,
  HttpMethod,
  KeyValue,
  RequestBody,
  ScriptOutcome,
} from '@/types/domain';
import type { RequestSettings } from '@/stores/app-store';
import { COMMANDS, invokeCmd, type CallApiOptions } from './tauri-commands';
import { buildFetchInit, type PreparedRequest } from './http-build';
import { applyOAuth2Token, fetchOAuth2Token } from './oauth2';

export interface SendInput {
  method: HttpMethod;
  url: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
  settings: RequestSettings;
  /** 可选的调用方中止信号（组件卸载、重发取消上一次在途请求）。 */
  signal?: AbortSignal;
}

// ---------------------------------------------------------------------------
// 共用辅助
// ---------------------------------------------------------------------------

/** 同样的扁平化，但从 `Array<[name, value]>` 起步（两种形态下 Rust 都把 header 序列化成元组列表）。 */
function tuplesToRecord(pairs: ReadonlyArray<readonly [string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of pairs) {
    if (out[k] === undefined) out[k] = v;
    else out[k] = `${out[k]}, ${v}`;
  }
  return out;
}

/** 尽力解析 JSON —— 失败就留 `bodyJson` 未定义（按约定不抛）。 */
function tryParseJson(text: string): unknown | undefined {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

// ---------------------------------------------------------------------------
// 错误形状构造器 —— 共用，好让 UI 不管跑的是哪条底层传输（Tauri IPC 还是
// serve-web HTTP）都看到同一个横幅
// ---------------------------------------------------------------------------

function timeoutResponse(input: SendInput, url: string, durationMs: number): ApiResponse {
  return {
    status: 0,
    statusText: 'Timeout',
    error: `Request timeout after ${input.settings.timeoutMs}ms`,
    durationMs,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

function abortedResponse(input: SendInput, url: string, durationMs: number): ApiResponse {
  return {
    status: 0,
    statusText: 'Aborted',
    error: 'aborted',
    durationMs,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

function networkErrorResponse(
  input: SendInput,
  url: string,
  durationMs: number,
  message: string,
): ApiResponse {
  return {
    status: 0,
    statusText: 'Network Error',
    error: message,
    durationMs,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

/** OAuth2 取 token 失败：不发请求，把后端的 message 交给 UI 的响应区横幅。 */
function oauth2ErrorResponse(input: SendInput, url: string, message: string): ApiResponse {
  return {
    status: 0,
    statusText: 'OAuth2 Error',
    error: message,
    durationMs: 0,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

// ---------------------------------------------------------------------------
// 后端结果信封 —— 对应 `crate::http_executor::HttpResult`（camelCase 字段，
// 见 Rust 结构体上的 `#[serde(rename_all = "camelCase")]`）。Tauri 命令与
// axum 的 `/api/http/send` 路由都返回这一形状，所以一个类型就能反序列化
// ---------------------------------------------------------------------------

interface RustHttpOk {
  status: number;
  statusText: string;
  durationMs: number;
  sizeBytes: number;
  headers: Array<[string, string]>;
  body: string;
  contentType: string;
  method: string;
  url: string;
  truncated: boolean;
}
interface RustHttpErr {
  kind: 'timeout' | 'network' | string;
  message: string;
}
interface RustHttpResult {
  ok: boolean;
  response?: RustHttpOk;
  error?: RustHttpErr;
}

/**
 * 后端一次发送的返回：传输层的信封 + 脚本的产出。
 *
 * `script` 缺席 = 链上没有任何脚本。脚本失败**不**改写 `http` 里的成败。
 */
interface RustSendOutcome {
  http: RustHttpResult;
  script?: ScriptOutcome | null;
}

/** 把任意 `HeadersInit` 形状转成 `[name, value]` 元组列表。 */
function initHeadersToTuples(h: HeadersInit): Array<[string, string]> {
  if (Array.isArray(h)) return h as Array<[string, string]>;
  if (typeof Headers !== 'undefined' && h instanceof Headers) {
    const out: Array<[string, string]> = [];
    h.forEach((v, k) => out.push([k, v]));
    return out;
  }
  if (h && typeof h === 'object') return Object.entries(h as Record<string, string>);
  return [];
}

// ---------------------------------------------------------------------------
// 公开入口 —— 一条传输路径，一个调用点。
// ---------------------------------------------------------------------------

/**
 * 把后端 `HttpResult` 信封转成渲染器的 `ApiResponse`。裸 URL 路径与 endpoint
 * 路径共用，好让两个界面产出逐字节相同的 UI 状态。
 */
function envelopeToApiResponse(r: RustHttpResult, input: SendInput, url: string): ApiResponse {
  if (!r.ok || !r.response) {
    // 往返成功，是服务端那个请求本身失败了
    const kind = r.error?.kind ?? 'network';
    const msg = r.error?.message ?? 'unknown';
    if (kind === 'timeout') {
      return timeoutResponse(input, url, 0);
    }
    return networkErrorResponse(input, url, 0, msg);
  }

  const resp = r.response;
  return {
    status: resp.status,
    statusText: resp.statusText || (resp.status >= 200 && resp.status < 300 ? 'OK' : 'Error'),
    durationMs: resp.durationMs,
    sizeBytes: resp.sizeBytes,
    headers: tuplesToRecord(resp.headers),
    body: resp.body,
    bodyJson: tryParseJson(resp.body),
    contentType: resp.contentType,
    method: input.method,
    url,
  };
}

/** 分发一次 `invokeCmd` 调用并把结果折成 `ApiResponse` + 脚本产出，绝不抛异常。 */
async function invokeAndMap(
  args: Record<string, unknown>,
  input: SendInput,
  url: string,
  options: CallApiOptions,
): Promise<{ response: ApiResponse; script: ScriptOutcome | null }> {
  try {
    const r = await invokeCmd<RustSendOutcome>(COMMANDS.httpRequest, args, options);
    return { response: envelopeToApiResponse(r.http, input, url), script: r.script ?? null };
  } catch (err) {
    // `invokeCmd` 会 reject 的情形：
    //   - Tauri IPC 失败（如 devtools 没接上）
    //   - serve-web HTTP 非 2xx / 网络不可达
    //   - WEB_ROUTES 缺该命令（只有加了 COMMANDS 却没配对应路由才会发生）
    //   - 调用方的 AbortSignal 中止（Tauri 形态 reject，web 形态抛 DOMException）
    // 中止单独识别，其余都落到通用 Network Error 横幅，UI 至少还有信息量
    const name = (err as { name?: string })?.name;
    const msg = (err as Error)?.message ?? String(err);
    if (name === 'AbortError' || /abort/i.test(msg)) {
      return { response: abortedResponse(input, url, 0), script: null };
    }
    console.error('[http.ts] backend invoke failed:', err);
    return { response: networkErrorResponse(input, url, 0, msg), script: null };
  }
}

/**
 * 经后端传输（`invokeCmd(COMMANDS.httpRequest, …)`）发送 `input` 描述的请求。
 *
 * 传输在模块加载时选定一次：
 *   - Tauri shell → `invoke()` IPC → Rust `http_executor::execute`
 *   - serve-web   → `POST /api/http/send` → 同一个 Rust executor
 *
 * 两种形态都打到同一个基于 `reqwest` 的 executor，所以超时、重定向、最大体积、
 * body 截断都在服务端发生，结果以 JSON 信封跨过边界。
 *
 * ⚠️ 这条路径允许调用方指定任意 URL，服务端因此成了代理。裸 URL 场景与
 * [`sendPreparedRequest`] 都一样——两者都收一个已完成的 URL，因为「前端组装
 * 整个请求、后端负责发」就意味着 URL 归前端所有。限制它的是 `OutboundPolicy`。
 *
 * 没有「裸浏览器 fetch」回退：所有受支持的发行形态前面都有真实后端，直连浏览器的
 * fetch 只在不带服务器的 Vite-only 模式下才有意义，而那种模式不发布。
 */
export async function sendRequest(input: SendInput): Promise<ApiResponse> {
  const built = buildFetchInit(input);
  // `buildFetchInit` 返回的 `RequestInit.headers` 可能是 `Headers | [][]`
  // | `Record`。统一归一成元组列表——Rust 反序列化 `Vec<(String, String)>`，
  // web 桥对元组列表原样 JSON 编码，这是两侧唯一都能用的形状
  const headers = initHeadersToTuples(built.init.headers ?? {});
  const body =
    typeof built.init.body === 'string' && built.init.body.length > 0 ? built.init.body : null;
  const options: CallApiOptions = input.signal ? { signal: input.signal } : {};

  const { response } = await invokeAndMap(
    {
      method: input.method,
      url: built.url,
      headers,
      body,
      timeoutMs: input.settings.timeoutMs,
      followRedirects: input.settings.followRedirects,
      maxResponseSizeKb: input.settings.maxResponseSizeKb,
    },
    input,
    built.url,
    options,
  );
  return response;
}

// ---------------------------------------------------------------------------
// Prepared-request 发送 —— 构造与执行终于是同一条路径
// ---------------------------------------------------------------------------

export interface SendPreparedInput {
  /**
   * `prepareRequest` 产出的已完成请求。构造归调用方（base URL join、
   * `{{var}}` 解析、header 排序、auth 归约），本函数只负责执行。
   */
  request: PreparedRequest;
  /**
   * `prepareRequest` 用的那份 auth。只为 OAuth2 而读：token 要发网络请求才拿得到，
   * 没法在那个纯函数里取，故注入发生在发送这一刻（见本函数的实现）。
   */
  auth?: AuthConfig;
  settings: RequestSettings;
  signal?: AbortSignal;
  /**
   * 端点 id。有它才装配得到脚本链（collection 的两段 + 端点自己的两段）；
   * 缺席 = 临时请求，没有脚本。
   */
  endpointId?: string;
  /**
   * 脚本看得到的环境。
   *
   * **与解析用的是同一份变量池**——脚本看到的必须是这次请求看到的那份，否则
   * `pm.environment.get('token')` 读到的与请求里替换进去的会不是同一个值。
   */
  env?: EnvSnapshot;
}

/** 一次发送的结果：响应 + 脚本的产出（没有脚本时 `script` 为 null）。 */
export interface SendPreparedResult {
  response: ApiResponse;
  script: ScriptOutcome | null;
}

/**
 * 发送一个已 prepare 的请求。
 *
 * 这个入口的意义在于：发出去的就是预览渲染的那个对象。组装归前端规则时，
 * cURL 预览与真实发送就成了两套靠手工对齐的实现，对不齐时用户看到的是
 * 「复制为 cURL」给出另一个请求；调一次 `prepareRequest` 再发其结果，
 * 这种分歧无从表达。
 *
 * header 走元组列表而非 `Record`：Rust 反序列化 `Vec<(String, String)>`，
 * web 桥（`POST /api/http/send`）对元组列表原样 JSON 编码，这是两侧唯一都能用
 * 的形状；`Record` 还会丢掉重名 header。
 *
 * OAuth2 是这里唯一在发送前还要做事的一档：token 那时才取得到，而把它写进
 * `prepared` 会让 `preparedToCurl` 把一个活凭据渲染进可复制的命令行、随剪贴板
 * 和工单流出去。故预览**刻意**不含 token —— 两者只差这一项，URL / header / body
 * 仍同源。
 *
 * ⚠️ 与 [`sendRequest`] 同一条注意事项：URL 由调用方提供，服务端因此成了它的
 * 代理，`OutboundPolicy` 依然生效。
 *
 * 绝不抛异常——失败以 `status: 0` 加可区分的 `statusText` 返回，与 `sendRequest`
 * 完全一致。
 */
export async function sendPreparedRequest(input: SendPreparedInput): Promise<SendPreparedResult> {
  const { request } = input;
  // 信封映射器与错误构造器只从这个形状读 `method` 与 `settings.timeoutMs`；
  // params/headers/body/auth 都已并入 `request`，所以下面的占位值永不被查
  const shape: SendInput = {
    method: request.method,
    url: request.url,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    settings: input.settings,
  };
  const options: CallApiOptions = input.signal ? { signal: input.signal } : {};

  // 真正发出去的那一份：`request` 是 cURL 预览共享的 `useMemo` 产物，注入只改副本。
  //
  // 不传 `force`：发请求走后端按需刷新（命中缓存就复用，过期才换），只有 UI 的
  // 「获取 Token」按钮才强制重取。
  let outbound = request;
  const auth = input.auth;
  if (auth?.kind === 'oauth2' && auth.oauth2) {
    try {
      const token = await fetchOAuth2Token(auth, { signal: input.signal });
      outbound = applyOAuth2Token(request, auth, token);
    } catch (err) {
      const name = (err as { name?: string })?.name;
      const msg = (err as Error)?.message ?? String(err);
      // 中止仍是中止：取 token 这一步被取消不该弹成 OAuth2 错误。
      if (name === 'AbortError' || /abort/i.test(msg)) {
        return { response: abortedResponse(shape, request.url, 0), script: null };
      }
      console.error('[http.ts] oauth2 token fetch failed:', err);
      return { response: oauth2ErrorResponse(shape, request.url, msg), script: null };
    }
  }

  return invokeAndMap(
    {
      method: outbound.method,
      url: outbound.url,
      // `outbound.headers` 已是 `[name, value][]`，原样传
      headers: outbound.headers,
      body: outbound.body,
      timeoutMs: input.settings.timeoutMs,
      followRedirects: input.settings.followRedirects,
      maxResponseSizeKb: input.settings.maxResponseSizeKb,
      // 端点 id 与环境：脚本链的装配与变量回写都靠它们。缺席时后端没有脚本链可跑。
      endpointId: input.endpointId ?? null,
      env: input.env ?? null,
    },
    shape,
    // 响应里的 `url` 报**注入前**的那份：它会随响应落进历史表，而 query 档的
    // token 就写在这个 URL 里。
    request.url,
    options,
  );
}
