/**
 * Common HTTP request headers, used by the KeyValueEditor's datalist
 * autocomplete on the Headers tab.
 *
 * The list is intentionally short and grouped by *intent*, not by spec
 * alphabet — when the user types `a` we want `Authorization` near the
 * top, not buried behind `Accept-CH`. Order inside the array is the
 * display order in the datalist (Chrome/Firefox honor it; Safari sorts
 * alphabetically — we accept that).
 *
 * Entries:
 *   - `name`: the literal header name. Will fill the key field.
 *   - `description`: one-line tooltip / dropdown subtitle. Shown when
 *     the editor renders a richer datalist; the basic <datalist> only
 *     shows `name`.
 *   - `placeholderValue?`: optional "fill the value" hint shown in the
 *     description (e.g. "Bearer xxx" for Authorization). We don't
 *     auto-fill — that would be too magical — but the hint nudges the
 *     user toward the right format.
 *   - `source?`: `builtin` for the curated list below, `recent` for
 *     names harvested from the user's own endpoints. Surfaced so the
 *     editor can label "最近用过" if it wants to render a richer UI
 *     than a plain <datalist>.
 */
export interface CommonHeader {
  name: string;
  description: string;
  placeholderValue?: string;
  source?: 'builtin' | 'recent';
}

/**
 * Build the merged suggestion list the editor consumes.
 *
 *   1. Walk every endpoint's headers and collect the *enabled* non-empty
 *      keys. These become the `recent` bucket — i.e. the user's own
 *      "global" headers, matching what Apifox surfaces in its global
 *      header autocomplete.
 *   2. Pull any recent names that aren't already in the built-in list
 *      to the front, preserving first-seen order (most-recent wins).
 *   3. Append the built-in list, deduped against the recent entries
 *      (case-insensitive on header name).
 *
 * Pure function — `endpoints` is the raw `endpoints` slice from the
 * store; filtering / sorting happens here so callers stay simple.
 */
export function buildHeaderSuggestions(
  endpoints: ReadonlyArray<{ headers: ReadonlyArray<{ key: string; enabled: boolean }> }>,
): CommonHeader[] {
  const seenLower = new Set<string>();
  const recent: CommonHeader[] = [];
  for (const ep of endpoints) {
    for (const h of ep.headers) {
      if (!h.enabled) continue;
      const name = h.key.trim();
      if (!name) continue;
      const lower = name.toLowerCase();
      if (seenLower.has(lower)) continue;
      seenLower.add(lower);
      recent.push({
        name,
        description: '最近使用过的 header',
        source: 'recent',
      });
    }
  }

  const builtinTail = COMMON_HEADERS.filter(
    (h) => !seenLower.has(h.name.toLowerCase()),
  ).map((h) => ({ ...h, source: 'builtin' as const }));

  return [...recent, ...builtinTail];
}

export const COMMON_HEADERS: readonly CommonHeader[] = [
  // ---------- identity / auth ----------
  {
    name: 'Authorization',
    description: '凭据令牌(Bearer / Basic / 自定义方案)',
    placeholderValue: 'Bearer eyJhbGciOi...',
  },
  {
    name: 'Cookie',
    description: '发送到服务器的 Cookie 字符串',
    placeholderValue: 'session=abc; user=42',
  },
  {
    name: 'X-API-Key',
    description: 'API Key(部分厂商约定头)',
  },
  {
    name: 'X-Auth-Token',
    description: '认证 token(部分厂商约定头)',
  },

  // ---------- content negotiation ----------
  {
    name: 'Content-Type',
    description: '请求体媒体类型',
    placeholderValue: 'application/json',
  },
  {
    name: 'Accept',
    description: '期望的响应媒体类型',
    placeholderValue: 'application/json',
  },
  {
    name: 'Accept-Language',
    description: '期望的语言(q 值可加权)',
    placeholderValue: 'zh-CN,zh;q=0.9,en;q=0.8',
  },
  {
    name: 'Accept-Encoding',
    description: '允许的压缩编码',
    placeholderValue: 'gzip, br, deflate',
  },
  {
    name: 'Accept-CH',
    description: '客户端提示,告知服务端可发送的偏好字段',
  },

  // ---------- cache / freshness ----------
  {
    name: 'Cache-Control',
    description: '缓存指令',
    placeholderValue: 'no-cache',
  },
  {
    name: 'If-Match',
    description: '条件请求:要求资源 ETag 匹配',
  },
  {
    name: 'If-None-Match',
    description: '条件请求:要求资源 ETag 不匹配(常用于 304)',
  },
  {
    name: 'If-Modified-Since',
    description: '条件请求:要求资源在该时间之后被修改',
  },
  {
    name: 'ETag',
    description: '资源标识符',
  },

  // ---------- cors / security ----------
  {
    name: 'Origin',
    description: '请求来源(浏览器跨域时自动加)',
  },
  {
    name: 'Referer',
    description: '上一个页面的 URL',
  },
  {
    name: 'X-Requested-With',
    description: '常用于 AJAX 标识,部分后端据此判断是否走 JSON 分支',
    placeholderValue: 'XMLHttpRequest',
  },
  {
    name: 'X-Forwarded-For',
    description: '代理链中的原始客户端 IP',
  },
  {
    name: 'X-Real-IP',
    description: '网关设置的原始客户端 IP',
  },

  // ---------- user agent ----------
  {
    name: 'User-Agent',
    description: '客户端标识',
    placeholderValue: 'simple-post/0.1',
  },

  // ---------- tracing / observability ----------
  {
    name: 'X-Request-Id',
    description: '请求唯一 ID,便于服务端日志关联',
  },
  {
    name: 'X-Correlation-Id',
    description: '分布式追踪 ID,跨服务关联',
  },
  {
    name: 'Traceparent',
    description: 'W3C Trace Context 父 span 信息',
  },

  // ---------- misc ----------
  {
    name: 'Host',
    description: '目标主机(由 URL 推导,通常无需手动设置)',
  },
  {
    name: 'Range',
    description: '请求资源的字节范围',
    placeholderValue: 'bytes=0-1023',
  },
];