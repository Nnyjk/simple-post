/**
 * 「可用工具清单」表格的分类。
 *
 * daemon 报来的是按字母排好的扁平 `string[]`。所以按注册表本就在用的工具名前缀
 *（`env_`、`endpoint_`…）分组——这个前缀不是额外叠上去的启发式：Rust 注册表里的
 * `REQUIRED_CAPABILITY` 与 `tool_table()` 认的也是同一套约定，所以这里的每一组都
 * 对应一个内聚的能力档。名字不匹配任何前缀的工具不算错，它落进「其他」而不是
 * 消失，这样缺前缀这件事是看得见的而不是静默的。
 */

export interface ToolGroup {
  /** React 列表与测试用的稳定 key。 */
  key: string;
  /** 渲染成该组行标题的可读标签。 */
  label: string;
  /** 工具名，已排序，使表格顺序在多次轮询间稳定。 */
  items: string[];
}

const CATEGORIES: ReadonlyArray<{ key: string; label: string; prefix: string }> = [
  // 「接口规格」排在最前：它是写侧的总入口，一次调用落一整份接口规范。
  { key: 'spec', label: '接口规格', prefix: 'spec_' },
  { key: 'env', label: '环境变量', prefix: 'env_' },
  { key: 'project', label: '项目', prefix: 'project_' },
  { key: 'module', label: '模块', prefix: 'module_' },
  { key: 'collection', label: '集合', prefix: 'collection_' },
  { key: 'endpoint', label: '接口', prefix: 'endpoint_' },
  { key: 'dynamic', label: '动态变量', prefix: 'dynamic_var_' },
  { key: 'api', label: '请求执行', prefix: 'api_' },
];

export const OTHER_GROUP_KEY = 'other';
export const OTHER_GROUP_LABEL = '其他';

/**
 * 把 `names` 拆进上面的分类，保持 CATEGORIES 的顺序，有未匹配项时把「其他」追加
 * 在最后。
 *
 * 没有成员的组会被丢掉——空标题只是噪声，而总数卡片头部已经交代了。
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
