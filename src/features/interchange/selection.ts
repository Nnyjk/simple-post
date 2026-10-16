/**
 * 勾选集合的纯运算。**刻意不放在组件里**：全选 / 全不选 / 反选与分组三态是这个弹窗
 * 唯一的真逻辑，而它没有任何 React 依赖，测起来不必起渲染器。
 */
import type { TransferNode } from '@/lib/interchange-types';

/** 一个分组下所有端点的 key，按树的声明顺序。 */
export function keysUnder(node: TransferNode): string[] {
  const out: string[] = [];
  for (const ep of node.endpoints) out.push(ep.key);
  for (const child of node.children) out.push(...keysUnder(child));
  return out;
}

/** 整棵树下的全部端点 key，按声明顺序。顺序稳定，反选与回填都靠它。 */
export function allKeys(groups: TransferNode[]): string[] {
  return groups.flatMap(keysUnder);
}

/** 分组的勾选状态：`all` 全选、`some` 部分、`none` 全不选。 */
export type GroupState = 'all' | 'some' | 'none';

export function groupState(node: TransferNode, selected: ReadonlySet<string>): GroupState {
  const keys = keysUnder(node);
  // 空分组既不是「全选」也不是「不选」——它没有可勾的东西，三态一律给 `none`，
  // 父复选框灰着。
  if (keys.length === 0) return 'none';
  let hits = 0;
  for (const k of keys) if (selected.has(k)) hits += 1;
  if (hits === 0) return 'none';
  return hits === keys.length ? 'all' : 'some';
}

/** 增删一批 key，返回**新的**集合：调用方的 state 不会被就地改。 */
function withKeys(
  selected: ReadonlySet<string>,
  keys: readonly string[],
  on: boolean,
): Set<string> {
  const next = new Set(selected);
  for (const k of keys) {
    if (on) next.add(k);
    else next.delete(k);
  }
  return next;
}

export const selectAll = (selected: ReadonlySet<string>, groups: TransferNode[]) =>
  withKeys(selected, allKeys(groups), true);

export const selectNone = (_selected: ReadonlySet<string>, _groups: TransferNode[]) =>
  new Set<string>();

/**
 * 反选：**在整棵树的 key 全集上取补集**。
 *
 * 分母固定是这棵树，不是「当前没选中的那些」——后者会让分母每次点击都变一次，
 * 用户连点两下就回到原样，反而以为按钮坏了。
 */
export const invert = (selected: ReadonlySet<string>, groups: TransferNode[]) => {
  const next = new Set<string>();
  for (const k of allKeys(groups)) {
    if (!selected.has(k)) next.add(k);
  }
  return next;
};

/** 分组行：全不选就全选，全选就全不选，部分则补满。 */
export function toggleGroup(node: TransferNode, selected: ReadonlySet<string>): Set<string> {
  const keys = keysUnder(node);
  const state = groupState(node, selected);
  return withKeys(selected, keys, state !== 'all');
}

/** 端点行：切换它自己。 */
export const toggleEndpoint = (key: string, selected: ReadonlySet<string>) => {
  const next = new Set(selected);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
};

/** 分组行上要不要显示「部分选中」。原生 checkbox 的 `indeterminate` 没有对应属性值。 */
export function isIndeterminate(node: TransferNode, selected: ReadonlySet<string>): boolean {
  return groupState(node, selected) === 'some';
}

export function countSelected(selected: ReadonlySet<string>, groups: TransferNode[]): number {
  let n = 0;
  for (const k of allKeys(groups)) if (selected.has(k)) n += 1;
  return n;
}
