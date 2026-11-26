import {
  applyRewrites,
  collapseUrlSlashes,
  joinBaseUrl,
  listVars,
  resolveVarsWithMissing,
} from '../url';
import { pathPlaceholders, substitutePathParams } from '../path-params';
import {
  assembleMultipart,
  multipartPartByteRanges,
  newMultipartBoundary,
  type MultipartPartByteRange,
} from '../multipart';
import { binaryContentType, parseMultipartParts, resolveBody } from '../body';
import { appendQueryString } from './query';
import { appendApikeyToQuery, resolveAuth } from './auth';
import { buildOrderedHeaders } from './headers';
import { buildPool, createMissingTracker } from './pool';
import { wellFormedDeep } from '../well-formed';
import { buildGraphQlPayload } from '../graphql';
import type { RequestProblem } from '../request-validation';
import type { BuildInput, PrepareRequestInput, PreparedRequest } from './types';

/**
 * 把用户可编辑的请求形状变成传输层需要的一切。全应用唯一组装请求的函数。
 *
 * 操作顺序（每一步的顺序都有讲究）：
 *
 *   1. 用路径前缀改写规则改写 `urlPath`——只改端点自己的 URL，模块地址自带的路径段不参与匹配；
 *   2. join `baseUrl` + `urlPath`——自带 scheme 的 path 优先，「自定义」的完整 URL 因此不会被前缀；
 *   3. 把路径里的 `{占位符}` 换成端点自己的路径参数——它们的值先解析 `{{var}}`，再按路径段编码；
 *   4. 在 join 后的 URL 上解析 `{{var}}`，模块地址里的变量才解析得到；
 *   5. 模块地址再解析一趟 `{{var}}`——缺值报告要指出缺的是哪个变量；
 *   6. 折叠 URL path 里的重复斜杠；
 *   7. 逐个解析启用参数的**值**并把存活者拼成查询串（编码，并按当前 URL 选 `?` 还是 `&`）；
 *   8. 追加 query 位置的 apikey；
 *   9. 构造有序 header 列表；
 *  10. 决定 body 是否包含（`mode !== 'none'` **且**原文非空），再按模式解析 body。
 *
 * multipart 的 boundary 在建 header 与 body 之前生成：Content-Type 与 body 用同一个值，两者
 * 不在同一步生成就会对不上。每个上线的字段都被模板化，body 也不例外；每个写下的 `{{var}}`
 * 解析不到就记录。
 *
 * body 的规则住在 `src/lib/body`：它与 Rust 的 `simple_post_core::body` 是同一套规则的两份实现
 * （模板化按字段做还是按整段做、空键算不算、产物是文本还是字节），改动两边必须同步。
 *
 * `protocol === 'graphql'` 时多一条支路：方法钉成 `POST`、body 换成由查询与 variables 组装出的
 * payload、Content-Type 钉成 `application/json`；端点自己的 body 设置不参与组装。
 */
export function prepareRequest(rawInput: PrepareRequestInput): PreparedRequest {
  // prepareRequest 先把用户写下的字符串归一成良构字符串：落单的代理项在 UTF-8 里没有对应
  // 字节，`encodeURIComponent` 遇到它直接抛 `URIError`，而请求是在渲染期组装的，异常一路
  // 到 React 根节点，整棵工作区树被卸载。
  const input = normalizeInput(rawInput);

  const missing = createMissingTracker();

  // 变量池在 `buildPool` 里合成一处，故 URL、参数、header、body 与鉴权看到的是同一份池。
  const pool = buildPool(input.env, input.dynamicVars);

  // GraphQL 是 HTTP 语义层：方法强制 POST，body 由查询与 variables 组装，端点自己那条 body
  // 设置不参与（切换协议不清空它，切回 HTTP 时原样还在）。query 与 variables 里的 `{{var}}`
  // 都在组装之前替换——两者都是用户可编辑的字段，与 URL / header 走同一套模板规则；MCP 的
  // `graphql_payload` 也替换 query，两边少一处就会让同一个端点发出不同的查询文本。
  const isGraphQl = input.protocol === 'graphql';
  const graphQlIssues: string[] = [];
  const problems: RequestProblem[] = [];
  let graphQlBody: string | null = null;
  if (isGraphQl) {
    const query = resolveVarsWithMissing(input.graphql?.query ?? '', pool);
    const variables = resolveVarsWithMissing(input.graphql?.variables ?? '', pool);
    missing.add(query.missing);
    missing.add(variables.missing);
    const built = buildGraphQlPayload(
      input.graphql ? { ...input.graphql, query: query.value, variables: variables.value } : null,
    );
    if (built.ok) graphQlBody = built.body;
    else {
      graphQlIssues.push(...built.problems);
      // 组装期的问题是发送侧的闸门。校验看到的是**替换前**的模板（占位符放行），
      // `variables: {"id": {{id}}}` + `id=abc` 那种替换后才坏掉的情况只有这里知道。
      problems.push(...built.problems.map((message) => ({ tab: 'graphql' as const, message })));
    }
  }

  const auth = resolveAuth(input.auth, pool, missing);

  // 路径前缀改写作用在**端点自己的 URL** 上，且在拼接之前：模块地址自带的路径段不参与
  // 匹配，主机也不动。规则按顺序匹配，命中首条即停止。改写后的路径随后与模块地址拼接。
  const rewritten = applyRewrites(input.urlPath, input.rewrites);
  const joined = joinBaseUrl(input.baseUrl, rewritten);

  // 路径参数的值在下面那趟 `{{var}}` 替换**之前**解析：路径参数的值要先编码，编码过的
  // `{{var}}` 就再也认不出来了，故值里的变量在这一步就地解析。
  //
  // 值里仍有没解析出来的 `{{var}}` 时，这个路径参数按「还没准备好」处理（不替换、计入缺值）：
  // 把 `%7B%7Btoken%7D%7D` 塞进路径，用户看不出缺的是哪个变量。
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

  // 模块地址自己再解析一趟 `{{var}}`：它可能是模板，缺值报告要指出缺的是哪个变量。
  const resolvedBase = resolveVarsWithMissing(input.baseUrl, pool);
  missing.add(resolvedBase.missing);

  // `collapseUrlSlashes` 放在替换之后、绝不之前：`{{var}}` → `/api` 才会把 `joinBaseUrl` 插入的
  // 分隔符变成 `//`，到这里真实 path 才确定。`collapseUrlSlashes` 不碰 query/fragment，
  // 所以参数还能在它之后追加
  const normalized = collapseUrlSlashes(url.value);

  const withParams = appendQueryString(normalized, input.params, pool, missing);
  const finalUrl = appendApikeyToQuery(withParams, auth);

  // `prepareRequest` 在建 header 之前生成 boundary：Content-Type 里的 boundary 与 body
  // 里的分隔符不是同一个字符串时，请求看起来成功而服务端解析失败
  const boundary = !isGraphQl && input.body.mode === 'multipart' ? newMultipartBoundary() : null;

  // `binary` 模式的 Content-Type 取文件自己的类型，故在建 header 之前先读出来。
  const binaryCt =
    !isGraphQl && input.body.mode === 'binary' ? binaryContentType(input.body.content) : null;

  const headers = buildOrderedHeaders(
    input.headers,
    input.body.mode,
    auth,
    pool,
    missing,
    boundary,
    binaryCt,
    isGraphQl ? 'application/json' : null,
  );

  // `resolveBody` 先按原文判断是否包含，再模板化——与 Rust 的 `resolve_endpoint` 同序。先模板化
  // 再判断会静默丢掉 body 只是一个解析为空串的变量的请求。
  //
  // `form` 与 `multipart` 逐个字段模板化，`json` / `xml` / `raw` 整段模板化，`binary` 不模板化
  // （文件字节不是模板）。
  const resolved = isGraphQl
    ? { text: null, base64: null, missing: [] as string[] }
    : resolveBody(input.body, pool);
  missing.add(resolved.missing);

  // multipart 在模板化之后才装配：`content` 存的就是分片数组的 JSON 文本，故它与其它 mode
  // 共用同一条模板化路径。
  //
  // 内容为空时**不装配**：那样会得到一个只有收尾分隔符的 body，而 Rust 侧在 `resolve_body`
  // 给出空 body 时根本不会走到装配那一步。两侧不一致时，「空 body」在一边发出去是一条空
  // multipart、在另一边发出去是完全没有 body。
  let body: string | null = graphQlBody;
  let bodyBase64: string | null = null;
  let multipartPartRanges: MultipartPartByteRange[] | undefined;
  if (!isGraphQl && (resolved.text !== null || resolved.base64 !== null)) {
    if (input.body.mode === 'multipart' && boundary !== null) {
      const parts = parseMultipartParts(resolved.text ?? '');
      const assembled = assembleMultipart(parts, boundary);
      body = assembled.text;
      bodyBase64 = assembled.base64;
      // 上传进度要折算到具体文件行上，故区间与 body 同一趟算：分母与真正发出去的字节数一致。
      multipartPartRanges = multipartPartByteRanges(parts, boundary);
    } else {
      body = resolved.text;
      bodyBase64 = resolved.base64;
    }
  }

  return {
    method: isGraphQl ? 'POST' : input.method,
    url: finalUrl,
    headers,
    body,
    bodyBase64,
    // GraphQL 缺查询 / variables 坏了没有专门的通道：它们与缺变量一起走 `missingVars`。但
    // `missingVars` 只是展示用的清单，**不是闸门**——校验期看到的是替换前的模板、占位符一律
    // 放行，`variables: {"id": {{id}}}` + `id=abc` 这种替换后才坏掉的情况 `validateRequest`
    // 根本看不见。发送 / 建连入口看的是 `problems`，非空就不发（否则会发出一个没有 body 的 POST）。
    missingVars: [...missing.list(), ...graphQlIssues],
    missingPathParams: substituted.missing,
    problems,
    // 变量池随结果一起走：OAuth2 取 token 时要用的正是这一份（见 `PreparedRequest.vars`）。
    vars: pool,
    multipartPartRanges,
  };
}

/**
 * 组装前的归一。二进制模式的 `content` 是一份文件描述，其中 base64 是文件本身的字节
 * （按构造只有 ASCII 字符），而那份字符串可能很大 —— 故这个模式的 `content` 不进归一，
 * 描述里会发出去的字段由 `parseBodyFile` 归一。
 */
function normalizeInput(input: PrepareRequestInput): PrepareRequestInput {
  if (input.body.mode !== 'binary') return wellFormedDeep(input);
  const { body } = input;
  const normalized = wellFormedDeep({ ...input, body: { ...body, content: '' } });
  return { ...normalized, body };
}

/**
 * `BuildInput` → `PrepareRequestInput`，base URL 为空串。`BuildInput.url` 一向是完整 URL，
 * 故这里的兼容适配器把它当作无 base 可 join 的 path；`joinBaseUrl` 在该输入上是恒等的
 * （空 base 返回 path，自带 scheme 的 path 原样返回）。
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
