/**
 * GraphQL 请求的组装规则。纯函数，不碰 store、不碰传输层：`prepareRequest` 用它决定 body，
 * `validateRequest` 用它决定要不要拦下发送。
 *
 * 与 Rust 的 `simple_post_core` 同一套规则：
 *   - 查询空白 → 组装失败；
 *   - `variables` 原文 `trim()` 为空 → payload 不含该键；否则解析成 JSON，必须是**对象**；
 *   - `operationName` 写进 payload 的是 `trim()` 之后的串；trim 后为空 → 不含该键；
 *   - payload 的键序是 query、variables、operationName，用紧凑 JSON 发出去；
 *   - variables 是对象时，对象键排序后输出（后端 `serde_json` 的键序）。
 */
import type { GraphQlRequest } from '@/types/domain';

/** 组装结果：成功时带 payload 文本，失败时带一条条给用户看的问题。 */
export type GraphQlPayload = { ok: true; body: string } | { ok: false; problems: string[] };

/**
 * `{{name}}` 占位符。与 `src/lib/request-validation.ts` 的 `PLACEHOLDER` 认同一套名字（`\w+`）。
 *
 * 校验期把占位符换成无害文本再解析，理由同 json body：带 `{{var}}` 的 variables 不是「坏了」，
 * 只是还没替换。组装期不做替换——那时调用方已经把变量解析过了，剩下的占位符就是解析不到的。
 */
const PLACEHOLDER = /\{\{\w+\}\}/g;

type ParsedVariables =
  { ok: true; value: Record<string, unknown> } | { ok: false; problem: string };

/**
 * 解析 variables 原文。`allowPlaceholders` 为校验期打开：带 `{{var}}` 的模板经两趟替换后仍能
 * 解析成对象即放行，真正发不出去的坏 JSON 才报错。
 */
function parseVariables(raw: string, allowPlaceholders: boolean): ParsedVariables {
  // 两趟替换：值位置换裸 `0`，键位置换带引号的 `"0"`。
  const candidates = allowPlaceholders
    ? [raw, raw.replace(PLACEHOLDER, '0'), raw.replace(PLACEHOLDER, '"0"')]
    : [raw];
  let lastError = '';
  for (const candidate of candidates) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(candidate);
    } catch (e) {
      lastError = (e as Error).message;
      continue;
    }
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return { ok: false, problem: 'GraphQL variables 必须是 JSON 对象' };
    }
    return { ok: true, value: parsed as Record<string, unknown> };
  }
  return { ok: false, problem: `GraphQL variables 不是合法 JSON：${lastError}` };
}

/**
 * variables 原文的问题；空原文返回 `null`（payload 不含该键）。编辑器用它就地提示，故
 * 默认按校验期规则放行占位符。
 */
export function variablesProblem(raw: string, allowPlaceholders = true): string | null {
  if (!raw.trim()) return null;
  const parsed = parseVariables(raw, allowPlaceholders);
  return parsed.ok ? null : parsed.problem;
}

/**
 * 把 variables 的值规范成稳定键序：对象键排序、数组逐元素递归、标量原样。
 *
 * 后端用 `serde_json::to_string` 序列化对象，键按字节序输出。不排序的话，同一份
 * `variables`（`{"b":1,"a":2}` 这种多键对象）在两侧发出字节不同、语义相同的载荷，
 * 与 Rust 的同一套规则就对不上了。
 */
export function canonicalJson(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalJson);
  if (value !== null && typeof value === 'object') {
    // 用 `fromEntries` 而不是逐个赋值：键名是用户写的，`__proto__` 不该改写原型。
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonicalJson((value as Record<string, unknown>)[key])]),
    );
  }
  return value;
}

function check(graphql: GraphQlRequest | null | undefined, allowPlaceholders: boolean): string[] {
  const problems: string[] = [];
  if (!(graphql?.query ?? '').trim()) problems.push('GraphQL 查询为空');
  const problem = variablesProblem(graphql?.variables ?? '', allowPlaceholders);
  if (problem) problems.push(problem);
  return problems;
}

/**
 * 这条 GraphQL 请求有哪些问题（给界面看的中文句子）。空数组 = 可以发。
 *
 * 校验期用：占位符放行，因为它们会在组装前被替换成真实值。
 */
export function graphqlProblems(graphql: GraphQlRequest | null | undefined): string[] {
  return check(graphql, true);
}

/**
 * 把 GraphQL 三段组装成请求 body。`graphql` 为 `null`（协议是 graphql 但没有配置）时按
 * 「查询为空」处理。
 *
 * 组装期用：variables 里的 `{{var}}` 应当已被调用方替换过，因此这里不做占位符放行——
 * 残留的占位符会让解析失败，正是需要拦下的情况。
 *
 * variables 解析失败或不是对象时返回 `ok:false`，**不产生 payload**：`prepareRequest` 把
 * 这几条问题记进 `missingVars`（不抛异常），`validateRequest` 在发送前拦下。合成一条
 * 「丢了 variables 的查询」去发，只会让用户看到一个与输入不符的服务端错误。
 */
export function buildGraphQlPayload(graphql: GraphQlRequest | null | undefined): GraphQlPayload {
  const problems = check(graphql, false);
  if (problems.length > 0) return { ok: false, problems };

  const payload: { query: string; variables?: Record<string, unknown>; operationName?: string } = {
    query: graphql!.query,
  };
  const rawVariables = (graphql!.variables ?? '').trim();
  if (rawVariables) {
    // `check` 已确认这是对象，这里的断言只是把类型收窄。对象内部按后端序列化后的键序
    // 排好；payload 自己那三个键仍按契约顺序（query、variables、operationName）。
    payload.variables = canonicalJson(JSON.parse(rawVariables)) as Record<string, unknown>;
  }
  // 名字在 GraphQL 里是标识符，首尾空白不可能是用户想要的：trim 只用来判空会在 payload 里
  // 留一个服务端认不出的名字，故写进去的是 trim 之后的串。
  const operationName = (graphql!.operationName ?? '').trim();
  if (operationName) payload.operationName = operationName;
  return { ok: true, body: JSON.stringify(payload) };
}
