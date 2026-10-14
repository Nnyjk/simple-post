/**
 * OAuth2 的前端一半：取一次 token（唯一的 I/O）与把它注入一份已 prepare 的请求
 * （纯函数）。取 token 本身在 Rust 侧（`crates/simple-post-core/src/oauth2.rs`），
 * 它同时服务 MCP 的 `api_call_endpoint` 与本应用的 UI，故解析、缓存、续期都在那里；
 * 这里只做两件渲染层自己的事：把结果挂到请求上，以及描述这次取到了什么。
 *
 * 注入**刻意**不发生在 `prepareRequest` 里：那个函数是纯的、无 I/O 的，而 token 要
 * 发网络请求才拿得到。把 I/O 塞进去会让 cURL 预览（同一个构造核）拿到一个有副作用的
 * 调用点。见 `http.ts` 的 `sendPreparedRequest`。
 */
import type { AuthConfig, OAuth2Auth, OAuth2TokenResult } from '@/types/domain';
import type { PreparedRequest } from './http-build';
import { COMMANDS, invokeCmd, type CallApiOptions } from './tauri-commands';

/**
 * `AuthConfig.oauth2` 缺省时的渲染形状。库里存着 `'{}'` 这样的默认行，MCP 也可能
 * 只写 `kind: 'oauth2'` 而不给载荷，所以读取端一律回落这里；**不要**据此回写 store
 * —— 那会把用户没配过的字段当成他配过的存下去。
 */
export const DEFAULT_OAUTH2: OAuth2Auth = {
  grantType: 'authorization_code',
  tokenUrl: '',
  clientId: '',
  clientSecret: '',
  clientAuth: 'basic',
  scope: '',
  authorizationUrl: '',
  redirectUri: '',
  codeVerifier: '',
  authCode: '',
  username: '',
  password: '',
  extraParams: [],
  tokenLocation: 'header',
  tokenParam: '',
};

/** query 档的参数名兜底，与 Rust 的 `OAuth2Auth::DEFAULT_TOKEN_PARAM` 同名同值。 */
export const DEFAULT_TOKEN_PARAM = 'access_token';

/** header 大小写不敏感，故查重按小写比；与 `http-build` 的 auth 派生 header 同一规则。 */
export function hasAuthorizationHeader(headers: ReadonlyArray<readonly [string, string]>): boolean {
  return headers.some(([k]) => k.toLowerCase() === 'authorization');
}

/**
 * 拼 query 位置的 token。键值都过 `encodeURIComponent`：access token 常见 base64url
 * 以外的形态（带 `+` / `=` / `.`），不编码会被 `&` 或 `+` 截断成一个残缺的值。
 */
export function buildTokenQuery(url: string, param: string, token: OAuth2TokenResult): string {
  const key = param.trim() === '' ? DEFAULT_TOKEN_PARAM : param;
  return `${url}${url.includes('?') ? '&' : '?'}${encodeURIComponent(key)}=${encodeURIComponent(token.accessToken)}`;
}

/**
 * `Authorization` 的取值：`<tokenType> <accessToken>`（RFC 6749 §7.1）。
 * `tokenType` 原样透传 IdP 回的值——少数是 `MAC` / `Token`，替它改成 `Bearer`
 * 就是打错请求；空串则只给裸 token，与 Rust 的 `auth_header_value` 同一条规则。
 */
export function buildAuthHeaderValue(token: OAuth2TokenResult): string {
  const type = token.tokenType.trim();
  return type === '' ? token.accessToken : `${type} ${token.accessToken}`;
}

/**
 * 把 token 注入一份已 prepare 的请求。**返回新对象，不改传入的**——`PreparedRequest`
 * 是 cURL 预览共享的 `useMemo` 产物，就地追加 header 会让预览在下一次渲染里
 * 悄悄多出一条 `Authorization`。
 *
 * 非 oauth2 的 auth 原样返回：本模块只认这一种注入点。
 *
 * header 档不覆盖用户手写的 `Authorization`（大小写不敏感），与 `http-build` 里
 * auth 派生 header 的处理一致：两条同名 header 在线上是两条，而 `fetch` 用 Record
 * 只会留最后一个，用户看到的与发出去的就对不上了。
 */
export function applyOAuth2Token(
  request: PreparedRequest,
  auth: AuthConfig,
  token: OAuth2TokenResult,
): PreparedRequest {
  if (auth.kind !== 'oauth2' || !auth.oauth2) return request;
  const cfg = auth.oauth2;

  if (cfg.tokenLocation === 'query') {
    return { ...request, url: buildTokenQuery(request.url, cfg.tokenParam, token) };
  }
  if (hasAuthorizationHeader(request.headers)) return request;
  return {
    ...request,
    headers: [
      ...request.headers,
      ['Authorization', buildAuthHeaderValue(token)] as [string, string],
    ],
  };
}

export interface FetchTokenOptions extends CallApiOptions {
  /** 忽略后端缓存强制重取；UI 的「获取 Token」按钮用。发请求的路径不传，走按需刷新。 */
  force?: boolean;
  /** 用户粘贴的原文，只用于 `implicit`（那里没有 token 端点可调）。 */
  manual?: string;
}

/**
 * 向后端要一份可注入的 token。失败时 reject（`invokeCmd` 的既有约定），调用方自行
 * 兜住——UI 显示错误横幅，`http.ts` 把它折成一个 `status: 0` 的响应。
 */
export async function fetchOAuth2Token(
  auth: AuthConfig,
  options: FetchTokenOptions = {},
): Promise<OAuth2TokenResult> {
  const args: Record<string, unknown> = { auth };
  if (options.manual !== undefined) args.manual = { pasted: options.manual };
  if (options.force) args.force = true;
  return invokeCmd<OAuth2TokenResult>(COMMANDS.oauth2Token, args, { signal: options.signal });
}
