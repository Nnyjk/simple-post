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

type ParsedVariables =
  { ok: true; value: Record<string, unknown> } | { ok: false; problem: string };

/** 把 variables 原文解析成对象；不是对象或不是 JSON 都算问题。 */
function parseVariables(raw: string): ParsedVariables {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (e) {
    return { ok: false, problem: `GraphQL variables 不是合法 JSON：${(e as Error).message}` };
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return { ok: false, problem: 'GraphQL variables 必须是 JSON 对象' };
  }
  return { ok: true, value: parsed as Record<string, unknown> };
}

/**
 * variables 原文的问题；空原文返回 `null`（payload 不含该键）。编辑器用它就地提示，故
 * 默认按校验期规则放行占位符。
 *
 * 校验期（`allowPlaceholders`）含 `{{var}}` 时整段不判形状：占位符可能整段引用一行
 * variables（`{{vars}}`）、落在值位置（`{"id": {{id}}}`）或键位置（`{{key}}: 1`），
 * 三种形态替换后是否成立只有组装期知道——那里拿到的是**替换后**的真实文本。校验放行的
 * 那些由 `PreparedRequest.problems` 裁决，发送前照样拦得住（见 `prepareRequest`）。
 * 没有占位符的原文照旧严格校验成 JSON 对象。
 */
export function variablesProblem(raw: string, allowPlaceholders = true): string | null {
  if (!raw.trim()) return null;
  if (allowPlaceholders && raw.includes('{{')) return null;
  const parsed = parseVariables(raw);
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
 * 这几条问题记进 `missingVars` 与 `problems`（不抛异常）。真正的闸门是 `problems`：校验看到
 * 的是替换前的模板（占位符放行），只有组装期看得见替换后的真实文本；不拦下就会发出一个没有
 * body 的 POST。合成一条「丢了 variables 的查询」去发，只会让用户看到一个与输入不符的服务端
 * 错误。
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
