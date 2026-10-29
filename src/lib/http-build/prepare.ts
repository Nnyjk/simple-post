import { collapseUrlSlashes, joinBaseUrl, listVars, resolveVarsWithMissing } from '../url';
import { pathPlaceholders, substitutePathParams } from '../path-params';
import { assembleMultipart, newMultipartBoundary } from '../multipart';
import { binaryContentType, parseMultipartParts, resolveBody } from '../body';
import { appendQueryString } from './query';
import { appendApikeyToQuery, resolveAuth } from './auth';
import { buildOrderedHeaders } from './headers';
import { buildPool, createMissingTracker } from './pool';
import type { BuildInput, PrepareRequestInput, PreparedRequest } from './types';

/**
 * 把用户可编辑的请求形状变成传输层需要的一切。全应用唯一组装请求的函数。
 *
 * 操作顺序（每一步的顺序都有讲究）：
 *
 *   1. join `baseUrl` + `urlPath`——自带 scheme 的 path 优先，「自定义」的完整 URL
 *      因此不会被前缀；
 *   2. 把路径里的 `{占位符}` 换成端点自己的路径参数——它们的值先解析 `{{var}}`，
 *      再按路径段编码；
 *   3. 在 join 后的 URL 上解析 `{{var}}`，环境自身 base URL 里的变量才解析得到；
 *   4. 逐个解析启用参数的**值**并把存活者拼成查询串（编码，并按当前 URL
 *      选 `?` 还是 `&`）；
 *   5. 追加 query 位置的 apikey；
 *   6. 构造有序 header 列表；
 *   7. 决定 body 是否包含（`mode !== 'none'` **且**原文非空），再按模式解析 body。
 *
 * multipart 的 boundary 在第 5 步之前生成：Content-Type 与 body 用同一个值，
 * 两者不在同一步生成就会对不上。
 *
 * 每个上线的字段都被模板化，body 也不例外；每个写下的 `{{var}}` 解析不到就记录。
 *
 * body 的规则住在 `src/lib/body`：它与 Rust 的 `simple_post_core::body` 是同一套规则的
 * 两份实现（模板化按字段做还是按整段做、空键算不算、产物是文本还是字节），改动两边
 * 必须同步。
 */
export function prepareRequest(input: PrepareRequestInput): PreparedRequest {
  const missing = createMissingTracker();

  // 变量池在 `buildPool` 里合成一处，故 URL、参数、header、body 与鉴权看到的是同一份池。
  const pool = buildPool(input.env, input.dynamicVars);

  const auth = resolveAuth(input.auth, pool, missing);

  const joined = joinBaseUrl(input.baseUrl, input.urlPath);

  // 路径参数的值在下面那趟 `{{var}}` 替换**之前**解析：路径参数的值要先编码，
  // 编码过的 `{{var}}` 就再也认不出来了，故值里的变量在这一步就地解析。
  //
  // 值里仍有没解析出来的 `{{var}}` 时，这个路径参数按「还没准备好」处理（不替换、
  // 计入缺值）：把 `%7B%7Btoken%7D%7D` 塞进路径，用户看不出缺的是哪个变量。
  const pathValues: Array<readonly [string, string]> = [];
  for (const name of pathPlaceholders(joined)) {
    const p = (input.pathParams ?? []).find((it) => it.enabled && it.key === name);
    // 只有空白的值按「还没填」处理：界面上的「必填」也是这么判的，说法要一致。
    if (!p || !p.value.trim()) continue;
    const r = resolveVarsWithMissing(p.value, pool);
    missing.add(r.missing);
    if (listVars(r.value).length > 0) continue;
    pathValues.push([name, r.value]);
  }
  const substituted = substitutePathParams(joined, pathValues);

  const url = resolveVarsWithMissing(substituted.url, pool);
  missing.add(url.missing);
  // `collapseUrlSlashes` 放在替换之后、绝不之前：`{{var}}` → `/api` 才会把 `joinBaseUrl`
  // 插入的分隔符变成 `//`，到这里真实 path 才确定。`collapseUrlSlashes` 不碰
  // query/fragment，所以参数还能在它之后追加
  const normalized = collapseUrlSlashes(url.value);

  const withParams = appendQueryString(normalized, input.params, pool, missing);
  const finalUrl = appendApikeyToQuery(withParams, auth);

  // `prepareRequest` 在建 header 之前生成 boundary：Content-Type 里的 boundary 与 body
  // 里的分隔符不是同一个字符串时，请求看起来成功而服务端解析失败
  const boundary = input.body.mode === 'multipart' ? newMultipartBoundary() : null;

  // `binary` 模式的 Content-Type 取文件自己的类型，故在建 header 之前先读出来。
  const binaryCt = input.body.mode === 'binary' ? binaryContentType(input.body.content) : null;

  const headers = buildOrderedHeaders(
    input.headers,
    input.body.mode,
    auth,
    pool,
    missing,
    boundary,
    binaryCt,
  );

  // `resolveBody` 先按原文判断是否包含，再模板化——与 Rust 的 `resolve_endpoint` 同序。
  // 先模板化再判断会静默丢掉 body 只是一个解析为空串的变量的请求。
  //
  // `form` 与 `multipart` 逐个字段模板化，`json` / `xml` / `raw` 整段模板化，
  // `binary` 不模板化（文件字节不是模板）。
  const resolved = resolveBody(input.body, pool);
  missing.add(resolved.missing);

  // multipart 在模板化之后才装配：`content` 存的就是分片数组的 JSON 文本，
  // 故它与其它 mode 共用同一条模板化路径
  //
  // 内容为空时**不装配**：那样会得到一个只有收尾分隔符的 body，而 Rust 侧在
  // `resolve_body` 给出空 body 时根本不会走到装配那一步。两侧不一致时，
  // 「空 body」在一边发出去是一条空 multipart、在另一边发出去是完全没有 body。
  let body: string | null = null;
  let bodyBase64: string | null = null;
  if (resolved.text !== null || resolved.base64 !== null) {
    if (input.body.mode === 'multipart' && boundary !== null) {
      const assembled = assembleMultipart(parseMultipartParts(resolved.text ?? ''), boundary);
      body = assembled.text;
      bodyBase64 = assembled.base64;
    } else {
      body = resolved.text;
      bodyBase64 = resolved.base64;
    }
  }

  return {
    method: input.method,
    url: finalUrl,
    headers,
    body,
    bodyBase64,
    missingVars: missing.list(),
    missingPathParams: substituted.missing,
  };
}

/**
 * `BuildInput` → `PrepareRequestInput`，base URL 为空串。
 *
 * `BuildInput.url` 一向是完整 URL，所以这里的兼容适配器把它当作无 base 可 join 的
 * path；`joinBaseUrl` 在该输入上是恒等的（空 base 返回 path，自带 scheme 的 path
 * 原样返回）。
 */
export function fromBuildInput(input: BuildInput): PrepareRequestInput {
  return {
    method: input.method,
    urlPath: input.url,
    baseUrl: '',
    params: input.params,
    headers: input.headers,
    body: input.body,
    auth: input.auth,
    env: input.env,
    pathParams: input.pathParams,
  };
}
