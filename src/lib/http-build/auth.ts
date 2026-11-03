import type { AuthConfig, OAuth2Auth } from '@/types/domain';
import { bytesToBase64 } from '@/lib/base64';
import { listVars, resolveVarsWithMissing } from '../url';
import type { MissingTracker } from './pool';

/**
 * OAuth2 配置里会被 `{{var}}` 替换的字段。替换本身发生在后端（Rust 侧的
 * `simple_post_core::oauth2`），渲染进程不替换：取 token 时后端才有变量池。
 *
 * 这份清单是渲染进程唯一需要知道的 OAuth2 侧规则——它决定哪些字段的变量进缺值报告。与
 * Rust 侧那份同一份清单，由 `fixtures/oauth2-config-parity.json` 钉住。
 */
export const OAUTH2_SUBSTITUTED_FIELDS = [
  'tokenUrl',
  'authorizationUrl',
  'clientId',
  'scope',
] as const;

/**
 * OAuth2 配置里没解析出来的变量名。只看**会被替换**的那四个字段：密钥与一次性输入
 * （`clientSecret` / `password` / `authCode`）不参与替换，故它们里面的 `{{var}}` 不是缺变量。
 *
 * 判据与 Rust 侧那份同一套：变量名是裸标识符，且变量池里没有它。池里有但取值是空串算已解析。
 */
export function oauth2MissingVars(cfg: OAuth2Auth, pool: Record<string, string>): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const field of OAUTH2_SUBSTITUTED_FIELDS) {
    for (const name of listVars(cfg[field])) {
      if (seen.has(name)) continue;
      // 用 `hasOwnProperty` 而不是 `in`：`in` 会撞上 `Object.prototype` 上的名字。
      if (Object.prototype.hasOwnProperty.call(pool, name)) continue;
      seen.add(name);
      out.push(name);
    }
  }
  return out;
}

/**
 * 解析 auth config 里的 `{{var}}` 并记录未解析项。返回新对象、不改传入值——auth config 存在
 * Zustand store 里，发送不能写它。
 *
 * 只扫选中的 `kind` 且只扫会上线的字段：Basic config 上残留的 `bearer` 串不属于这次请求，
 * 报它的变量纯属噪声。
 *
 * **OAuth2 的配置不替换**，只把没解析出来的变量报出来：替换由后端在取 token 之前做（规则见
 * `OAUTH2_SUBSTITUTED_FIELDS` 的说明）。渲染进程报一次，用户在发请求之前就看得见「缺哪个
 * 变量」。
 */
export function resolveAuth(
  auth: AuthConfig,
  pool: Record<string, string>,
  missing: MissingTracker,
): AuthConfig {
  if (auth.kind === 'bearer') {
    const r = resolveVarsWithMissing(auth.bearer ?? '', pool);
    missing.add(r.missing);
    return { ...auth, bearer: r.value };
  }
  if (auth.kind === 'basic' && auth.basic) {
    const u = resolveVarsWithMissing(auth.basic.username, pool);
    const p = resolveVarsWithMissing(auth.basic.password, pool);
    missing.add(u.missing);
    missing.add(p.missing);
    return {
      ...auth,
      basic: { username: u.value, password: p.value },
    };
  }
  if (auth.kind === 'apikey' && auth.apikey) {
    const k = resolveVarsWithMissing(auth.apikey.key, pool);
    const v = resolveVarsWithMissing(auth.apikey.value, pool);
    missing.add(k.missing);
    missing.add(v.missing);
    return {
      ...auth,
      apikey: { ...auth.apikey, key: k.value, value: v.value },
    };
  }
  if (auth.kind === 'oauth2' && auth.oauth2) {
    missing.add(oauth2MissingVars(auth.oauth2, pool));
  }
  return auth;
}

/**
 * 为 HTTP Basic 认证把 `user:pass` 做 base64。走 `TextEncoder` 是为了先把含非 ASCII 的凭据
 * （中文用户名、emoji 密码）按 UTF-8 编码成字节再 base64——浏览器的事实惯例，Express 等多数
 * 服务端按裸字节解码这个 header。
 */
export function b64Basic(user: string, pass: string): string {
  const raw = `${user}:${pass}`;
  return bytesToBase64(new TextEncoder().encode(raw));
}

/** 把 `inLocation` 为 `'query'` 的 `apikey` 追加到 URL。 */
export function appendApikeyToQuery(url: string, auth: AuthConfig): string {
  if (auth.kind !== 'apikey' || !auth.apikey || auth.apikey.inLocation !== 'query') {
    return url;
  }
  const { key, value } = auth.apikey;
  if (!key) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${encodeURIComponent(key)}=${encodeURIComponent(value)}`;
}
