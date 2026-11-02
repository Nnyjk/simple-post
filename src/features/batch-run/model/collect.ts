/**
 * 集合子树的端点清单 —— 按**树的显示顺序**。
 *
 * 次序规则与界面渲染的那一套一致：容器**直接持有**的接口先出，然后才是它的子集合，
 * 逐个递归展开。它不是纯粹的深度优先前序——界面把一个容器自己的接口画在它的子集合
 * **之上**，故批量执行的次序与用户在树上看到的行序相同。
 *
 * Rust 侧的 `db::nodes::ordered_subtree_endpoints` 是同一套规则的另一半实现。两侧各
 * 写一遍遍历，故顺序规则由 `fixtures/collection-order-parity.json` 钉住，测试读同一
 * 份文件。
 */
import type { ResourceNode } from '@/types/domain';

/**
 * 取某个容器子树下的全部端点。
 *
 * `nodes` 是工作区的全部节点（打平的数组），`rootId` 指向一个模块或集合。根自己就是
 * 端点时，返回它自己。根不存在时返回空列表。
 */
export function collectSubtreeEndpoints(
  nodes: readonly ResourceNode[],
  rootId: string,
): ResourceNode[] {
  const root = nodes.find((n) => n.id === rootId);
  if (!root) return [];
  if (root.kind === 'endpoint') return [root];

  // 父 → 子。同一个父下的兄弟按 `(sortOrder, id)` 排，与服务端
  // `list_by_root` / `list_children` 的 `ORDER BY sort_order ASC, id ASC` 一致。
  const children = new Map<string, ResourceNode[]>();
  for (const node of nodes) {
    if (!node.parentId) continue;
    const bucket = children.get(node.parentId);
    if (bucket) bucket.push(node);
    else children.set(node.parentId, [node]);
  }
  for (const bucket of children.values()) {
    bucket.sort((a, b) => a.sortOrder - b.sortOrder || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  }

  const out: ResourceNode[] = [];
  // 访问过的容器。父子关系由用户拖拽决定，数据里出现环时这里不能绕圈。
  const seen = new Set<string>();
  // 栈里放「待展开的容器」，第一个就是根自己——根自己直接持有的接口也要出。
  const stack: ResourceNode[] = [root];
  while (stack.length > 0) {
    const container = stack.pop()!;
    if (seen.has(container.id)) continue;
    seen.add(container.id);
    // 本容器直接持有的接口先出。
    for (const child of children.get(container.id) ?? []) {
      if (child.kind === 'endpoint') out.push(child);
    }
    // 再展开它的子集合（逆序压栈，弹出时才是正序）。
    const collections = (children.get(container.id) ?? []).filter((c) => c.kind === 'collection');
    for (let i = collections.length - 1; i >= 0; i--) stack.push(collections[i]);
  }
  return out;
}

/**
 * 某个容器子树下的接口数量与子集合数量，报告标题要用。
 *
 * 子集合数只数**直接**的子集合：标题说的是「这个集合下面有几个子集合」，而递归总数
 * 会随嵌套深度变化，读起来反而不准。
 */
export function countSubtree(
  nodes: readonly ResourceNode[],
  rootId: string,
): { endpoints: number; collections: number } {
  const endpoints = collectSubtreeEndpoints(nodes, rootId).length;
  let collections = 0;
  for (const node of nodes) {
    if (node.kind === 'collection' && node.parentId === rootId) collections++;
  }
  return { endpoints, collections };
}
