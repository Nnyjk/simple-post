import type { AuthConfig, BodyMode, KeyValue } from '@/types/domain';
import { resolveVarsWithMissing } from '../url';
import { b64Basic } from './auth';
import type { MissingTracker } from './pool';

/**
 * 按 body mode 自动补 Content-Type（`none` 与 `multipart` 返回 `null`）。
 *
 * multipart 的 Content-Type 含 boundary，而 boundary 是执行层装配时现生成的，故这里给不了：
 * 由 `http_executor` 在发请求前就地改写（用户写过同名的就换值，没写过就追加）。
 * 渲染进程不生成 boundary，也不碰这条头。
 *
 * `binaryContentType` 是 `binary` 模式下文件自己的 MIME 类型：整个请求体就是一个文件时，用文件
 * 那个类型才描述得了它。它只在 `bodyMode === 'binary'` 时被读，其余模式传 `null`。
 */
function autoContentType(mode: BodyMode, binaryContentType?: string | null): string | null {
  switch (mode) {
    case 'json':
      return 'application/json';
    case 'form':
      return 'application/x-www-form-urlencoded';
    case 'xml':
      return 'application/xml';
    case 'raw':
      return 'text/plain';
    case 'binary':
      return binaryContentType || 'application/octet-stream';
    default:
      return null;
  }
}

/**
 * 有序 header 列表：先是用户 header，再是自动 Content-Type（缺失才补）与
 * Accept，最后是 auth 派生的 header（`Authorization` 或 header 位置的 apikey）。查重大小写不敏感
 * ——HTTP header 名本就大小写不敏感。`apikey.inLocation === 'query'` 不出 header，调用方通过
 * `appendApikeyToQuery` 拼到 URL 上。
 *
 * **multipart 不在这里补 Content-Type。** 它的值含 boundary，而 boundary 由执行层装配时现生成
 * （`simple-post-core::multipart`），这条头也在那时就地改写。故这里既不给它定位置，也不生成值；
 * `PreparedRequest.headers` 里看不到 multipart 的 Content-Type，而真正发出去的那一条由后端报回来
 * （`requestHeaders`）。
 */
export function buildOrderedHeaders(
  headers: readonly KeyValue[],
  bodyMode: BodyMode,
  auth: AuthConfig,
  pool: Record<string, string>,
  missing: MissingTracker,
  binaryContentType?: string | null,
  /**
   * GraphQL 的 Content-Type。payload 的形状由组装器决定，用户的 body 设置不参与，故这里
   * 直接钉住 `application/json`；它的位置与 body 模式推导出的那条相同（Accept 之前）。
   */
  overrideContentType?: string | null,
): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const h of headers) {
    if (!h.enabled || !h.key) continue;
    const r = resolveVarsWithMissing(h.value, pool);
    missing.add(r.missing);
    out.push([h.key, r.value]);
  }

  // GraphQL 的 payload 一律是 JSON，由组装期决定值：用户写过就就地替换，没写过就补一条。
  // multipart 不在此列——它的 Content-Type 要等执行层生成 boundary。
  const fixedCt = overrideContentType ?? null;
  const existingCt = out.findIndex(([k]) => k.toLowerCase() === 'content-type');

  if (fixedCt !== null) {
    // 就地替换用户自己写过的那条 Content-Type 的**值**，header 名保留用户的大小写：GraphQL 的
    // payload 是 JSON，用户那条值在结构上描述不了它——留着就是「发得错」。
    if (existingCt !== -1) {
      out[existingCt] = [out[existingCt]![0], fixedCt];
    } else {
      // GraphQL 的值在组装前就定了，位置与 body 模式推导出的 Content-Type 相同：
      // Accept 之前。Rust 侧同样先放它再加默认 Accept（见 fixtures/resolve-parity.json）。
      out.push(['Content-Type', fixedCt]);
    }
  } else {
    // `buildOrderedHeaders` 按 body.mode 补 Content-Type：用户已设过（不分大小写）就不加；mode
    // 蕴含 body 时它照补，内容空不空由 `init.body` 那一步单独管。
    const ct = autoContentType(bodyMode, binaryContentType);
    if (ct && existingCt === -1) {
      out.push(['Content-Type', ct]);
    }
  }

  // `buildOrderedHeaders` 补默认的 Accept，让服务端知道我们要干净地解析 JSON
  if (!out.some(([k]) => k.toLowerCase() === 'accept')) {
    out.push(['Accept', 'application/json']);
  }

  // `buildOrderedHeaders` 补 auth 派生的 header：用户已有同名（不分大小写）时跳过——fetch 用
  // Record 只会留最后一个值，而 cURL 里两条 `-H 'Authorization: …'` 服务端看到的是 header 列表。
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
