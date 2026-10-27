import type { AuthConfig } from '@/types/domain';
import { resolveVarsWithMissing } from '../url';
import type { MissingTracker } from './pool';

/**
 * 解析 auth config 里的 `{{var}}` 并记录未解析项。返回新对象、不改传入值——
 * auth config 存在 Zustand store 里，发送不能写它。
 *
 * 只扫选中的 `kind` 且只扫会上线的字段：Basic config 上残留的 `bearer` 串不属于
 * 这次请求，报它的变量纯属噪声。
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
  return auth;
}

/**
 * 为 HTTP Basic 认证把 `user:pass` 做 base64。走 `TextEncoder` 是为了先把含非 ASCII
 * 的凭据（中文用户名、emoji 密码）按 UTF-8 编码成字节再 base64——浏览器的事实惯例，
 * Express 等多数服务端按裸字节解码这个 header。
 *
 * `btoa` 由 DOM lib 提供，已在项目 tsconfig 的 `"lib": ["ES2022", "DOM", "DOM.Iterable"]` 里。
 */
export function b64Basic(user: string, pass: string): string {
  const raw = `${user}:${pass}`;
  const bytes = new TextEncoder().encode(raw);
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]!);
  return btoa(bin);
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
