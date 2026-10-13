/**
 * Categorisation for the 「可用工具清单」 table.
 *
 * The daemon reports a flat, alphabetically sorted `string[]` of 44 tool
 * names. Rendering that as one `<pre>` made the card a wall of text with
 * no shape — you could not tell that the tools come in families, and
 * reading "there are 44 of these" told you nothing about *what* they do.
 *
 * So the list is grouped by the tool-name prefix the registry already
 * uses (`env_`, `endpoint_`, …). That prefix is not a heuristic layered
 * on top: it is the same convention `REQUIRED_CAPABILITY` and
 * `tool_table()` key off in the Rust registry, so a group here always
 * matches one coherent capability tier. Adding a tool whose name does not
 * match any prefix is not an error — it lands in 「其他」 rather than
 * disappearing, which is what makes the missing-prefix case visible
 * instead of silent.
 */

export interface ToolGroup {
  /** Stable key for React lists and for tests. */
  key: string;
  /** Human label rendered as the group's row heading. */
  label: string;
  /** Tool names, sorted, so the table order is stable across polls. */
  items: string[];
}

const CATEGORIES: ReadonlyArray<{ key: string; label: string; prefix: string }> = [
  { key: 'env', label: '环境变量', prefix: 'env_' },
  { key: 'project', label: '项目', prefix: 'project_' },
  { key: 'module', label: '模块', prefix: 'module_' },
  { key: 'collection', label: '集合', prefix: 'collection_' },
  { key: 'endpoint', label: '接口', prefix: 'endpoint_' },
  { key: 'baseUrl', label: '服务地址', prefix: 'base_url_def_' },
  { key: 'api', label: '请求执行', prefix: 'api_' },
];

export const OTHER_GROUP_KEY = 'other';
export const OTHER_GROUP_LABEL = '其他';

/**
 * Split `names` into the categories above, preserving CATEGORIES order
 * and appending 「其他」 last when something did not match.
 *
 * Groups with no members are dropped — an empty heading is noise, and
 * the count is already implied by the card header's total.
 */
export function groupTools(names: readonly string[]): ToolGroup[] {
  const buckets = new Map<string, string[]>();
  for (const c of CATEGORIES) buckets.set(c.key, []);
  buckets.set(OTHER_GROUP_KEY, []);

  for (const name of names) {
    const match = CATEGORIES.find((c) => name.startsWith(c.prefix));
    buckets.get(match ? match.key : OTHER_GROUP_KEY)!.push(name);
  }

  const groups: ToolGroup[] = [];
  for (const c of CATEGORIES) {
    const items = buckets.get(c.key)!;
    if (items.length > 0) {
      groups.push({ key: c.key, label: c.label, items: items.sort() });
    }
  }
  const other = buckets.get(OTHER_GROUP_KEY)!;
  if (other.length > 0) {
    groups.push({ key: OTHER_GROUP_KEY, label: OTHER_GROUP_LABEL, items: other.sort() });
  }
  return groups;
}
