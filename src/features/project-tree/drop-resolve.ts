import type { ResourceNode } from '@/types/domain';
import { mayUnder, type PlacementScope } from '@/lib/node-placement';
import type { DragPayload, DropZone, TreeNodeKind } from './tree-types';

/**
 * 一次放置被拒绝的理由。每条理由都对应一个具体的问题，故都单独命名 —— 测试能逐条断言
 * 「哪一种组合被哪一条规则挡住」，而不只是断言「什么都没发生」。
 */
export type DropRejection =
  /** 没有可用的拖拽载荷。 */
  | 'no-drag'
  /** 落回被拖的那一行自己。 */
  | 'self'
  /** 目标与被拖的节点不在同一个项目。 */
  | 'cross-project'
  /** 父子类型矩阵不接受这个落点。 */
  | 'type'
  /** 落点会把节点嵌进自己的后代。 */
  | 'cycle'
  /** 目标行已经不在树里。 */
  | 'unknown-target';

/**
 * 一次放置的结论。成功时给出新父级与新槽位：`parentId: null` 表示「项目顶层」，
 * 而项目顶层只放得下模块。
 */
export type DropDecision =
  { ok: true; parentId: string | null; index: number } | { ok: false; reason: DropRejection };

/**
 * 目标行对当前这次拖拽开放的放置区。
 *
 * 两个区各自独立：某一行可能只开放其中一个。模块行对集合与接口的拖拽就只开放
 * `into` —— 它的兄弟作用域是项目顶层，那里只放得下模块。
 */
export interface DropBands {
  /** 能否嵌进这一行，即成为它的子节点。 */
  into: boolean;
  /** 能否成为这一行的兄弟，即落进它自己的父级里、排它前后。 */
  beside: boolean;
}

/** 落点的目标行。 */
export interface DropTarget {
  kind: TreeNodeKind;
  id: string;
}

/**
 * 算出光标落在目标行的哪一块。
 *
 * 三种分档：
 *
 *   - 两个区都开放（容器行对容器或接口的拖拽）：25/50/25 —— 上四分位是「放在它上面」，
 *     下四分位是「放在它下面」，中间一半是「嵌进去」。
 *   - 只开放 `into`（模块行对集合或接口的拖拽）：整行都是「嵌进去」。
 *   - 只开放 `beside`（模块行对模块的拖拽、接口行对接口的拖拽）：朴素 50/50。
 *
 * 矩形缺失或高度为 0 时谈比例没有意义，故解析成一个确定的方向：只开放 `into` 时
 * 回落到 `into`，其余回落到 `above`。
 *
 * 返回 `null` 表示这一行不接受这次拖拽 —— 调用方据此不画落点提示，并把光标显示成
 * 「不可放」。
 */
export function zoneFromRowPosition(
  rect: { top: number; height: number } | null | undefined,
  clientY: number,
  bands: DropBands,
): DropZone | null {
  if (!bands.into && !bands.beside) return null;
  if (!rect || rect.height <= 0) {
    return bands.into && !bands.beside ? 'into' : 'above';
  }
  const ratio = (clientY - rect.top) / rect.height;
  if (!bands.into) return ratio < 0.5 ? 'above' : 'below';
  if (!bands.beside) return 'into';
  if (ratio < 0.28) return 'above';
  if (ratio > 0.72) return 'below';
  return 'into';
}

/**
 * 目标行的父级作用域 —— 「落进目标自己的父级里」这件事，父级是谁。
 *
 * 模块的父级是项目（`parent_id = NULL`），故它的作用域是 `'project'`；集合与接口的
 * 父级是节点，作用域就是那个父节点的类型。
 */
function scopeOf(
  target: DropTarget,
  targetNode: ResourceNode,
  nodes: ResourceNode[],
): PlacementScope {
  if (target.kind === 'module') return 'project';
  if (!targetNode.parentId) return 'project';
  const parent = nodes.find((n) => n.id === targetNode.parentId);
  return parent ? parent.kind : 'project';
}

/**
 * 目标行对一次拖拽开放的放置区。两条规则都来自父子类型矩阵。
 *
 *   - `into`：被拖的类型能不能坐在目标类型之下。
 *   - `beside`：被拖的类型能不能坐在目标自己父级的作用域里。
 *
 * 一处刻意收窄：**接口行只接受接口的拖拽**。那一行的手势是「在容器内重排接口」，
 * 而树把接口与集合分开渲染（集合永远排在接口之后），故把集合拖到接口旁边虽然符合
 * 类型矩阵，落点却不是用户看到的位置。想把集合挪到模块里的接口旁边，拖到模块行上。
 */
export function bandsFor(
  dragKind: TreeNodeKind,
  target: DropTarget,
  targetNode: ResourceNode,
  nodes: ResourceNode[],
): DropBands {
  const scope = scopeOf(target, targetNode, nodes);
  const into = mayUnder(dragKind, target.kind);
  let beside = mayUnder(dragKind, scope);
  if (target.kind === 'endpoint' && dragKind !== 'endpoint') beside = false;
  return { into, beside };
}

/**
 * 目标行在这次拖拽下的放置区。目标行不在树里时返回 `null`。
 */
export function zoneFor(
  drag: DragPayload,
  target: DropTarget,
  nodes: ResourceNode[],
  rect: { top: number; height: number } | null,
  clientY: number,
): DropZone | null {
  const targetNode = nodes.find((n) => n.id === target.id);
  if (!targetNode) return null;
  const bands = bandsFor(drag.kind, target, targetNode, nodes);
  return zoneFromRowPosition(rect, clientY, bands);
}

/**
 * 一次拖放的全部输入。`nodes` 必须是当前打开的项目下的节点 —— 树的拖放不跨项目，
 * 而「项目顶层」是所有项目共用的作用域。
 */
export interface DropInput {
  /** 被拖的节点。`null` 表示没有可用的拖拽载荷。 */
  drag: DragPayload | null;
  /** 落点行。 */
  target: DropTarget;
  /** 目标行的 client rect。 */
  rect: { top: number; height: number } | null;
  /** 光标的视口 Y 坐标。 */
  clientY: number;
  /** 当前打开的项目下的节点。 */
  nodes: ResourceNode[];
  /** 当前打开的项目。 */
  activeProjectId: string | null;
}

/**
 * 解析一次放置：给出新父级与新槽位，或者给出被拒绝的理由。
 *
 * 两种落点：
 *
 *   - **`into`** —— 成为目标行的子节点，落在它的子列表末尾。
 *   - **`above` / `below`** —— 成为目标行的兄弟，即落进目标自己的父级里、排它前后。
 *
 * 槽位按「目标自己的父级里、去掉被移动者之后」的同级序列算 —— 插入动作也发生在那一份
 * 序列上（被移动者要先从原位摘掉）。拿含被移动者的序列算槽位会差一位：在同一个父级里
 * 往下拖时，会落到目标后面再一格。
 */
export function resolveDrop(input: DropInput): DropDecision {
  const { drag, target, rect, clientY, nodes, activeProjectId } = input;
  if (!drag) return { ok: false, reason: 'no-drag' };

  // 落回自己：节点 id 全树唯一，故被拖的 id 里有目标这一项就够了。
  if (drag.ids.includes(target.id)) return { ok: false, reason: 'self' };

  const targetNode = nodes.find((n) => n.id === target.id);
  if (!targetNode) return { ok: false, reason: 'unknown-target' };

  // 跨项目：被拖的每一个节点都要与目标同属一个项目。
  for (const id of drag.ids) {
    const node = nodes.find((n) => n.id === id);
    if (!node || node.rootId !== targetNode.rootId) {
      return { ok: false, reason: 'cross-project' };
    }
  }

  const bands = bandsFor(drag.kind, target, targetNode, nodes);
  const zone = zoneFromRowPosition(rect, clientY, bands);
  if (!zone) return { ok: false, reason: 'type' };

  if (zone === 'into') {
    if (!bands.into) return { ok: false, reason: 'type' };
    if (wouldCycleInto(drag.ids, target.id, nodes)) return { ok: false, reason: 'cycle' };
    const children = siblingsOf(target.id, nodes).filter((n) => !drag.ids.includes(n.id));
    return { ok: true, parentId: target.id, index: children.length };
  }

  if (!bands.beside) return { ok: false, reason: 'type' };
  const parentId = targetNode.parentId;
  if (parentId !== null && wouldCycleInto(drag.ids, parentId, nodes)) {
    return { ok: false, reason: 'cycle' };
  }
  const siblings = siblingsOf(parentId, nodes, activeProjectId).filter(
    (n) => !drag.ids.includes(n.id),
  );
  const idx = siblings.findIndex((n) => n.id === target.id);
  if (idx < 0) return { ok: false, reason: 'unknown-target' };
  return { ok: true, parentId, index: zone === 'above' ? idx : idx + 1 };
}

/**
 * 某个父级下的同级节点，按显示顺序。
 *
 * `parentId: null` 是项目顶层 —— 即模块。须同时按 `rootId` 收窄，因为 `null` 是所有
 * 项目共用的顶层。
 */
function siblingsOf(
  parentId: string | null,
  nodes: ResourceNode[],
  activeProjectId: string | null = null,
): ResourceNode[] {
  return nodes
    .filter((n) => n.parentId === parentId && (parentId !== null || n.rootId === activeProjectId))
    .sort((a, b) => a.sortOrder - b.sortOrder);
}

/**
 * 成环守卫：目标不能是源本身或源的任何后代，否则会把节点嵌进自己。
 *
 * 沿 `parentId` 遍历而不特判集合：模块被拖到其内部的集合上，与集合被拖到自己的子节点
 * 上是同一种环。
 */
function wouldCycleInto(movingIds: string[], parentId: string, nodes: ResourceNode[]): boolean {
  if (movingIds.includes(parentId)) return true;
  const childIndex = new Map<string, string[]>();
  for (const n of nodes) {
    if (!n.parentId) continue;
    const bucket = childIndex.get(n.parentId);
    if (bucket) bucket.push(n.id);
    else childIndex.set(n.parentId, [n.id]);
  }
  const blocked = new Set<string>(movingIds);
  const queue = [...movingIds];
  while (queue.length > 0) {
    const id = queue.shift()!;
    for (const kid of childIndex.get(id) ?? []) {
      if (!blocked.has(kid)) {
        blocked.add(kid);
        queue.push(kid);
      }
    }
  }
  return blocked.has(parentId);
}
