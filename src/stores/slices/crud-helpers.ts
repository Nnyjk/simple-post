/**
 * 本目录下 CRUD 各子工厂共用的纯助手。
 *
 * 它们刻意放在 store 侧而不是 `@/lib/tree-index`：每一个都只有一两个调用方，
 * 且全在 CRUD 切片内部，把只用一次的助手提升成共享模块只会给其他所有导入方
 * 扩大那个模块的表面积。`indexByParent` 相反——它确实共用，所以子工厂仍从
 * `@/lib/tree-index` 导入它。
 *
 * 除了 `invokeWithRollback` 之外，下面所有东西都是不碰 store 的纯计算，
 * 这正是它们能在 `set()` 回调里安全调用的原因。
 */
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import type { OpenTab, EndpointDraft } from '../store-types';

/**
 * 每次乐观写入的尾巴：调守护进程，失败时记日志并把本地 state 回滚。
 *
 * 日志前缀和"你记得回滚了吗？"这个问题由这里统一回答，调用方只提供这一次
 * 写入的"回滚"具体是什么。
 *
 * `rollback` 是回调而不是值，因为回滚形状确实不同：有些是必须跑在**当前**
 * state 上的 updater（`set((s) => …)`），有些是乐观写入之前捕获的快照对象。
 * 只接受单一形式的助手会迫使另一类改写。
 *
 * 注意它刻意**不**把错误吞成返回值——需要在失败时拿到值的调用方（各 create
 * 动作）保留自己的 try/catch 并返回 `null`。
 */
export async function invokeWithRollback(
  label: string,
  invoke: () => Promise<unknown>,
  rollback: () => void,
): Promise<void> {
  try {
    await invoke();
  } catch (e) {
    console.error(`[store] ${label} failed`, e);
    rollback();
  }
}

/** 重排后重新编号一个切片的 sortOrder。 */
export function renumber<T extends { sortOrder: number }>(items: readonly T[]): T[] {
  return items.map((it, idx) => ({ ...it, sortOrder: idx }));
}

/**
 * 一个节点及其下方所有后代。
 *
 * 沿 `parentId` 做 BFS，带一个已访问集合。这个集合不是防御性冗余：数据里出现
 * 环会让这里永远转下去，而 UI 是 store 与手工编辑过的 `store.db` 之间唯一的
 * 屏障。
 */
export function collectSubtree(
  rootId: string,
  childrenIndex: Map<string | null, ResourceNode[]>,
): Set<string> {
  const out = new Set<string>();
  const queue: string[] = [rootId];
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (out.has(id)) continue;
    out.add(id);
    for (const kid of childrenIndex.get(id) ?? []) queue.push(kid.id);
  }
  return out;
}

/**
 * 丢掉底层实体已被移除的标签。返回新的标签表、新的活动标签 id（活动标签被移除
 * 时取前一个邻居），以及一个"是否有变化"的标记。
 */
export function filterTabsAfterRemoval(
  tabs: readonly OpenTab[],
  activeId: string | null,
  closeSet: ReadonlySet<string>,
): { tabs: OpenTab[]; activeId: string | null; changed: boolean } {
  const nextTabs: OpenTab[] = [];
  for (const t of tabs) {
    if (closeSet.has(`${t.kind}:${t.id}`)) continue;
    nextTabs.push(t);
  }
  if (nextTabs.length === tabs.length) {
    return { tabs: tabs.slice(), activeId, changed: false };
  }

  let nextActive = activeId;
  if (activeId !== null && !nextTabs.some((t) => t.id === activeId)) {
    const oldIdx = tabs.findIndex((t) => t.id === activeId);
    // 优先前一个标签（左邻居），再退到后一个，最后 null。
    const fallback = tabs[oldIdx - 1] ?? tabs[oldIdx + 1] ?? nextTabs[nextTabs.length - 1] ?? null;
    nextActive = fallback ? fallback.id : null;
  }
  return { tabs: nextTabs, activeId: nextActive, changed: true };
}

/**
 * 删除一棵子树时该关掉的标签键。
 *
 * 标签按它所显示的东西的 id 作键，而 `module` / `collection` / `endpoint`
 * 标签的 id 就是它的节点 id——所以扫一遍被移除的 id 就同时回答了三者。project
 * 是另一回事：它们不是节点，由 project 动作负责关。
 */
export function tabKeysForSubtree(
  nodes: readonly ResourceNode[],
  removed: ReadonlySet<string>,
  scope: 'project' | 'node',
  targetId: string,
): Set<string> {
  const close = new Set<string>();
  for (const n of nodes) {
    if (removed.has(n.id)) close.add(`${n.kind}:${n.id}`);
  }
  if (scope === 'project') close.add(`project:${targetId}`);
  return close;
}

/**
 * 把 endpoint 草稿转成落盘它的 `updateEndpointDetail` 补丁。
 *
 * 两条保存路径共用——HttpTester 的 Save 按钮与脏关闭对话框的"保存"——所以
 * 它们不会分叉：给 `EndpointDraft` 新增的字段两边都自动带上，因为这个映射由
 * 草稿自身的键驱动，而不是手写的字段表。
 *
 * 每个草稿字段在 `EndpointDetail` 里都有同名对应，正是这个 1:1 让强制转换成立。
 * 若某字段需要在进来的路上做转换，这个前提就不成立，那时该字段应在这里显式列出。
 */
export function draftToPatch(draft: EndpointDraft): Partial<EndpointDetail> {
  const patch: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(draft)) {
    if (value !== undefined) patch[key] = value;
  }
  return patch as Partial<EndpointDetail>;
}

/** 乐观新建 endpoint 时起步用的 `EndpointDetail`。 */
export function blankDetail(
  nodeId: string,
  partial?: Partial<Omit<EndpointDetail, 'nodeId'>>,
): EndpointDetail {
  return {
    nodeId,
    method: 'GET',
    url: '',
    environmentId: null,
    baseUrlOverride: null,
    params: [],
    pathParams: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags: [],
    ...partial,
  };
}
