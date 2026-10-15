/**
 * web 形态的 bearer 缓存。`serve-web` 可能带 `--password=<token>` /
 * `SIMPLE_POST_WEB_PASSWORD` 启动，渲染层必须给它发出的每个 `/api/*` 与 `/mcp` 请求
 * 附上 `Authorization: Bearer <token>`。bearer 存在 `localStorage` 以跨刷新存活，
 * SPA 引导则在首屏从 URL query string 里捡起：
 *
 *   https://host/sp/?token=<x>     ← 反向代理的鉴权交接
 *   https://host/                   ← 之后的访问，读 localStorage
 *
 * 安全取舍：`localStorage` 对同源 XSS 可读。这是 Postman / Insomnia / dbx 这类
 * 独立客户端（无按用户会话）同样接受的暴露面。
 */ export const WEB_BEARER_STORAGE_KEY = 'simple-post.web.bearer';

export function writeBearerToStorage(token: string): void {
  try {
    window.localStorage.setItem(WEB_BEARER_STORAGE_KEY, token);
  } catch {
    /* 隐私模式 / 配额 —— 退回仅 URL 鉴权 */
  }
}

export function clearBearerFromStorage(): void {
  try {
    window.localStorage.removeItem(WEB_BEARER_STORAGE_KEY);
  } catch {
    /* 忽略 */
  }
}

/** 从 localStorage 读 bearer。在 Node 侧测试（无 `window`）或用户尚未登录时返回 `null`。 */
export function readBearerFromStorage(): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(WEB_BEARER_STORAGE_KEY);
  } catch {
    return null;
  }
}

/**
 * 一次性引导：URL 带 `?token=<x>` 时持久化它（覆盖旧值 —— 用户刚轮换了凭据）
 * 并抹掉 query，免得 bearer 落进浏览器历史 / Referer 头。返回该 token（或 null）。
 */
export function ingestBearerFromUrl(search: string = window.location.search): string | null {
  if (typeof window === 'undefined') return null;
  const params = new URLSearchParams(search);
  const candidate = params.get('token');
  if (!candidate) return null;
  writeBearerToStorage(candidate);
  // 抹掉 query，免得 bearer 泄进历史 / referrer。
  const clean = window.location.pathname + window.location.hash;
  window.history.replaceState(null, '', clean);
  return candidate;
}
