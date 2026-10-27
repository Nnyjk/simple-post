import type { AuthConfig, BodyMode, KeyValue } from '@/types/domain';
import { resolveVarsWithMissing } from '../url';
import { multipartContentType } from '../multipart';
import { b64Basic } from './auth';
import type { MissingTracker } from './pool';

/**
 * 按 body mode 自动补 Content-Type（`none` 与 `multipart` 返回 `null`）。
 *
 * multipart 的 Content-Type 含 boundary，而 boundary 在 `prepareRequest` 里现生成，
 * 故它由那条路径单独处理（见 `buildOrderedHeaders`）。
 */
export function autoContentType(mode: BodyMode): string | null {
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
 * 有序 header 列表：先是用户 header，再是自动 Content-Type（缺失才补，multipart
 * 例外）与 Accept，最后是 auth 派生的 header（`Authorization` 或 header 位置的
 * apikey）。查重大小写不敏感——HTTP header 名本就大小写不敏感。
 *
 * `apikey.inLocation === 'query'` 不出 header，调用方通过 `appendApikeyToQuery`
 * 拼到 URL 上。
 *
 * **multipart 的 Content-Type 位置与其它模式不同。** 它的值含 boundary，而 boundary
 * 每次请求现生成，故它不能与别的自动 Content-Type 那样在 header 阶段决定位置。
 * Rust 侧（`variable_fill::build_headers` + `mcp::http_ops::multipart::apply`）的做法是：
 * multipart 模式在 header 阶段**不**补 Content-Type；装配 body 的那一步若发现用户
 * 写过 Content-Type 就地替换它的值，没写过就追加到列表**末尾**。本函数照同一规则做，
 * 故两侧的头部顺序逐条相同。
 */
export function buildOrderedHeaders(
  headers: readonly KeyValue[],
  bodyMode: BodyMode,
  auth: AuthConfig,
  pool: Record<string, string>,
  missing: MissingTracker,
  boundary: string | null,
): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const h of headers) {
    if (!h.enabled || !h.key) continue;
    const r = resolveVarsWithMissing(h.value, pool);
    missing.add(r.missing);
    out.push([h.key, r.value]);
  }

  const isMultipart = bodyMode === 'multipart';
  const multipartCt = isMultipart && boundary !== null ? multipartContentType(boundary) : null;
  const existingCt = out.findIndex(([k]) => k.toLowerCase() === 'content-type');

  if (multipartCt !== null) {
    // `buildOrderedHeaders` 就地替换用户自己写过的那条 Content-Type 的**值**，header 名
    // 保留用户的大小写。其余模式的 body 是用户手写的，用户那条 Content-Type 有机会描述它；
    // multipart 的 body 由 boundary 框出来，用户那条值在结构上描述不了它 —— 留着就是
    // 「发得错」：请求发得出去、服务端解析不出分片，而用户看不出异常。
    if (existingCt !== -1) {
      out[existingCt] = [out[existingCt]![0], multipartCt];
    }
  } else {
    // `buildOrderedHeaders` 按 body.mode 补 Content-Type：用户已设过（不分大小写）就不加；
    // 只要 mode 蕴含 body 就补，内容空不空由 `init.body` 那一步单独管。
    const ct = autoContentType(bodyMode);
    if (ct && existingCt === -1) {
      out.push(['Content-Type', ct]);
    }
  }

  // `buildOrderedHeaders` 补默认的 Accept，让服务端知道我们要干净地解析 JSON
  if (!out.some(([k]) => k.toLowerCase() === 'accept')) {
    out.push(['Accept', 'application/json']);
  }

  // `buildOrderedHeaders` 补 auth 派生的 header：用户已有同名（不分大小写）时跳过——
  // fetch 用 Record 只会留最后一个值，而 cURL 里两条 `-H 'Authorization: …'` 服务端
  // 看到的是 header 列表而非覆盖。
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

  // multipart 且用户没写过 Content-Type 时，`buildOrderedHeaders` 把它追加到列表**末尾**，
  // 与 Rust 侧 `multipart::apply` 的追加位置一致。
  if (multipartCt !== null && existingCt === -1) {
    out.push(['Content-Type', multipartCt]);
  }

  return out;
}
