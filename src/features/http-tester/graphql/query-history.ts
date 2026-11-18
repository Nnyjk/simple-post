/**
 * 从响应历史里挑出 GraphQL 查询，给「查询历史」列表用。纯函数，不碰 store。
 *
 * 判据是请求体：GraphQL 请求的 body 一定是 `{"query": …}`，而普通 JSON 请求体里出现 `query`
 * 这种键也很常见（搜索条件之类），故必须**同时**是对象且 `query` 是字符串才算。
 */

/** 查询历史的一条。 */
export interface QueryHistoryEntry {
  query: string;
  /** 格式化后的 variables 原文；请求里没有 variables 时为空串。 */
  variables: string;
  operationName: string;
}

/**
 * 入参只要求历史行带一个请求快照，且只读 body：调用方传整个响应历史行也满足这个形状，
 * 但本模块不依赖 store 的类型。
 */
export interface QueryHistorySource {
  request?: { body: string | null } | null;
}

/**
 * 按 `query` 文本去重，保留入参顺序（调用方给的是新 → 旧），最多取 `limit` 条。
 */
export function pickQueryHistory(
  entries: readonly QueryHistorySource[],
  limit = 10,
): QueryHistoryEntry[] {
  const seen = new Set<string>();
  const picked: QueryHistoryEntry[] = [];
  for (const entry of entries) {
    if (picked.length >= limit) break;
    const body = entry.request?.body;
    if (typeof body !== 'string' || body.trim() === '') continue;

    let parsed: unknown;
    try {
      parsed = JSON.parse(body);
    } catch {
      continue;
    }
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) continue;
    const record = parsed as Record<string, unknown>;
    if (typeof record.query !== 'string' || record.query.trim() === '') continue;
    if (seen.has(record.query)) continue;
    seen.add(record.query);

    const variables = record.variables;
    picked.push({
      query: record.query,
      variables:
        variables !== null && typeof variables === 'object' && !Array.isArray(variables)
          ? JSON.stringify(variables, null, 2)
          : '',
      operationName:
        typeof record.operationName === 'string' && record.operationName.trim() !== ''
          ? record.operationName
          : '',
    });
  }
  return picked;
}
