import { useState, type DragEvent as ReactDragEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { DragPayload, DropZone, TargetKind } from '../tree-types';
import { DND_MIME, decodeDrag, encodeDrag } from '../tree-utils';

/**
 * 模块级的在途拖拽暂存。部分合成事件驱动（尤其是 Playwright 的 `dragAndDrop`）会触发
 * HTML5 的 dragstart / dragover / drop 序列，却不传递 dragstart 期间 `setData` 写入的
 * `DataTransfer` 载荷。这里镜像一份载荷，使 `makeDropHandler` 在 drop 时仍能取回拖拽上下文。
 * 真实浏览器会正常填充 dataTransfer，这里只用于那些会丢数据的环境。
 */
let lastDragPayload: DragPayload | null = null;

/**
 * 把应用内的拖拽载荷写到原生 dataTransfer。同时写一份 `text/plain`：某些浏览器在
 * 完全没有文本载荷时拒绝启动拖拽，单靠自定义 mime 并不总是够。
 */
function setDataTransfer(e: ReactDragEvent, payload: DragPayload) {
  const json = encodeDrag(payload);
  e.dataTransfer.setData(DND_MIME, json);
  e.dataTransfer.setData('text/plain', json);
  e.dataTransfer.effectAllowed = 'move';
  lastDragPayload = payload;
}

/**
 * 区域计算，写成 (行矩形, 光标, 拖拽类型能否嵌于此) 的纯函数。
 *
 * 从 `computeZoneFromEvent` 中拆出，使 25/50/25 与 50/50 的规则无需 DOM 即可测试 ——
 * 这是放置判定中唯一会静默退化而无人察觉的一环（见 `computeZoneFromEvent` 关于矩形来源
 * 的说明）。
 *
 * `rect` 是行自身的 client rect；`supportsInto` 是 `canNestInto(dragKind, targetKind)`。
 * 没有它时 —— 叶节点行，或拖拽类型不能嵌在此处 —— 该行保持朴素 50/50，永远不会答出
 * `into`。
 *
 * 矩形缺失或高度为 0 时谈比例没有意义，故解析为 `above`：它是在任何语境下都有意义、
 * 也是唯一不会把节点嵌到非法位置的那个区域。
 */
export function zoneFromRowPosition(
  rect: { top: number; height: number } | null | undefined,
  clientY: number,
  supportsInto: boolean,
): DropZone {
  if (!rect || rect.height <= 0) return 'above';
  const ratio = (clientY - rect.top) / rect.height;
  if (!supportsInto) return ratio < 0.5 ? 'above' : 'below';
  if (ratio < 0.28) return 'above';
  if (ratio > 0.72) return 'below';
  return 'into';
}

export interface TreeDndBindings {
  /** The active project's nodes — already filtered by `rootId`. */
  nodes: ResourceNode[];
  activeProjectId: string | null;
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => void;
}

/**
 * 树的全部拖放管线：载荷编码、跨项目守卫、放置索引计算、above/into/below 区域解析与
 * 实时放置提示。状态与 store 值由 `ProjectTree` 传入 —— 本 hook 不持有任何 store
 * selector。
 *
 * 节点落到哪，由目标的 `parentId` 加上服务端强制执行的类型矩阵决定。
 */
export function useTreeDnd({ nodes, activeProjectId, moveNodes }: TreeDndBindings) {
  /**
   * 拖拽进行中当前放置目标的实时指示器。无拖拽时为 `null`。以 `${kind}:${id}` 为键，
   * 故多个目标可同时高亮（例如父节点的 `into` 高亮 + 目标行上的一条线）。
   */
  const [dropHint, setDropHint] = useState<{
    key: string;
    zone: DropZone;
  } | null>(null);

  /**
   * 解析潜在放置的 (源, 目标) 项目 id 对。放置会跨越项目边界时返回 false
   * （规则：「任何拖拽都不能使目标脱离项目」）。
   */
  const sameProjectGuard = (
    drag: DragPayload,
    target: { kind: TargetKind; id: string },
  ): boolean => {
    const projectIdOf = (kind: TargetKind, id: string): string | undefined => {
      if (kind === 'project') return id;
      // 其余类型都是节点，而每个节点都记着自己的归属项目 —— 整条阶梯只是一次查找。
      return nodes.find((n) => n.id === id)?.rootId;
    };
    const targetProject = projectIdOf(target.kind, target.id);
    if (!targetProject) return false;
    for (const id of drag.ids) {
      if (projectIdOf(drag.kind, id) !== targetProject) return false;
    }
    return true;
  };

  /**
   * 本项目内 `parentId` 的兄弟节点，按显示顺序。
   *
   * `parentId: null` 是项目顶层 —— 即模块。须同时按 `rootId` 收窄，因为 `null` 是所有
   * 项目共用的顶层。
   */
  const siblingsOf = (parentId: string | null): ResourceNode[] =>
    nodes
      .filter((n) => n.parentId === parentId && (parentId !== null || n.rootId === activeProjectId))
      .sort((a, b) => a.sortOrder - b.sortOrder);

  /**
   * 计算被拖拽项应插入到新父级里的索引。需沿可见树找到正确的兄弟项，因为 move 动作里
   * 的 `index` 指的是目标已排序子列表内的位置。
   */
  const computeTargetIndex = (
    drag: DragPayload,
    target: { kind: TargetKind; id: string },
    zone: DropZone,
  ): number | null => {
    if (zone === 'into') {
      // 插到目标子列表末尾。模块装集合与接口，集合装接口与更多集合 —— 两种情形都只需一个
      // 计数，因为 move 动作只重排它插入其中的那几行。
      if (target.kind === 'module' || target.kind === 'collection') {
        return siblingsOf(target.id).length;
      }
      return null;
    }
    // above / below：插到目标行之前 / 之后，位于目标自己的兄弟之中。
    if (target.kind === 'module' || target.kind === 'collection' || target.kind === 'endpoint') {
      const targetNode = nodes.find((n) => n.id === target.id);
      if (!targetNode) return null;
      const siblings = siblingsOf(targetNode.parentId);
      const idx = siblings.findIndex((n) => n.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    return null;
  };

  /**
   * 成环守卫：目标不能是源本身或其任何后代，否则会把节点嵌进自己。
   *
   * 沿 `parentId` 遍历而不特判集合：模块被拖到其内部的集合上，与集合被拖到自己的子节点
   * 上是同一种环。
   */
  const wouldCycleInto = (movingIds: string[], targetId: string): boolean => {
    if (movingIds.includes(targetId)) return true;
    const childIndex = new Map<string, string[]>();
    for (const n of nodes) {
      if (n.parentId) {
        const bucket = childIndex.get(n.parentId);
        if (bucket) bucket.push(n.id);
        else childIndex.set(n.parentId, [n.id]);
      }
    }
    const blocked = new Set<string>(movingIds);
    const queue = [...movingIds];
    while (queue.length > 0) {
      const id = queue.shift()!;
      for (const k of childIndex.get(id) ?? []) {
        if (!blocked.has(k)) {
          blocked.add(k);
          queue.push(k);
        }
      }
    }
    return blocked.has(targetId);
  };

  /**
   * drop 处理器工厂。按行绑定，好让每一行自己决定是否接受放置（以及插到哪个索引）。
   */
  const makeDropHandler = (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const raw = e.dataTransfer.getData(DND_MIME) || e.dataTransfer.getData('text/plain');
    let drag = decodeDrag(raw);
    // 载入 dataTransfer 失败时改用镜像：部分合成拖拽驱动（Playwright）不会在
    // dragstart → drop 之间传递 dataTransfer，由 `setDataTransfer` 写入的
    // `lastDragPayload` 镜像覆盖这种情形。
    if (!drag) drag = lastDragPayload;
    setDropHint(null);
    lastDragPayload = null;
    if (!drag) return;
    if (drag.ids.includes(target.id) && drag.kind === target.kind) return;
    if (!sameProjectGuard(drag, target)) return;

    // 模块是项目顶层，没有节点父级，故只能在兄弟之间重排。
    if (drag.kind === 'module') {
      if (target.kind !== 'module') return;
      const idx = computeTargetIndex(drag, target, computeZoneFromEvent(e, drag, target));
      if (idx === null) return;
      moveNodes(drag.ids, null, idx);
      return;
    }

    if (target.kind === 'module') {
      // 模块同时接受集合**和**接口 —— 不需要自己集合的接口直接落在它上面。其余（另一个
      // 模块）服务端的类型矩阵会拒绝，故在这里先拒，免得乐观状态显示一个必然 400 的移动。
      if (drag.kind !== 'collection' && drag.kind !== 'endpoint') return;
      const zone = computeZoneFromEvent(e, drag, target);
      const idx =
        zone === 'into' ? siblingsOf(target.id).length : computeTargetIndex(drag, target, zone);
      if (idx === null) return;
      moveNodes(drag.ids, target.id, idx);
      return;
    }

    if (target.kind === 'collection') {
      const zone = computeZoneFromEvent(e, drag, target);
      if (zone === 'into') {
        if (drag.kind === 'collection' && wouldCycleInto(drag.ids, target.id)) return;
        moveNodes(drag.ids, target.id, siblingsOf(target.id).length);
        return;
      }
      // above/below：作为目标的兄弟落下，保留目标的父级。这是「脱离集合」的手势 —— 嵌套
      // 集合被拖到其父级旁边时落在其侧。
      //
      // `parentId` 一个分支都不用就覆盖两种情形：顶层集合的父级*就是*它的模块节点，嵌套
      // 集合的父级是它上面那个集合。这正是统一 parent 列的意义。
      const targetNode = nodes.find((n) => n.id === target.id);
      if (!targetNode || !targetNode.parentId) return;
      if (drag.kind === 'collection' && wouldCycleInto(drag.ids, target.id)) return;
      const idx = computeTargetIndex(drag, target, zone);
      if (idx === null) return;
      moveNodes(drag.ids, targetNode.parentId, idx);
      return;
    }

    // target.kind === 'endpoint' —— 落在某个接口旁边。只有接口走这里：move 会让被拖行
    // 成为目标的兄弟，而本分支实现的手势（「在集合的接口之间重排」）对容器没有意义。想把
    // 集合挪到模块级接口旁，改为拖到模块行上即可。
    if (drag.kind !== 'endpoint') return;
    const targetNode = nodes.find((n) => n.id === target.id);
    if (!targetNode || !targetNode.parentId) return;
    const idx = computeTargetIndex(drag, target, computeZoneFromEvent(e, drag, target));
    if (idx === null) return;
    moveNodes(drag.ids, targetNode.parentId, idx);
  };

  /**
   * 被拖拽类型能否**嵌套进**给定目标类型。驱动容器行上的 25/50/25（对比 50/50）分档。
   *
   * 规则 —— `ResourceNodeType::may_under` 的镜像：
   *   - 模块永不可嵌入（用户定的「模块无法被嵌入集合」规则），故拖拽类型是模块时
   *     `into` 永不成立。
   *   - 集合可嵌在模块或另一个集合之下。
   *   - 接口可嵌在模块或集合之下。
   */
  const canNestInto = (dragKind: TargetKind, targetKind: TargetKind): boolean => {
    if (targetKind === 'module') return dragKind === 'collection' || dragKind === 'endpoint';
    if (targetKind === 'collection') return dragKind === 'collection' || dragKind === 'endpoint';
    return false;
  };

  /**
   * 由 DragEvent 计算 DropZone，依据光标 Y 相对该行的位置。被拖拽类型能嵌进目标时
   * （见 `canNestInto`）容器用 25/50/25 分档；否则（或对接口这类叶行）用朴素 50/50。
   *
   * rect 取自 `e.currentTarget` —— 处理器所绑定的那个行元素 —— 而非当参数传进来。
   * 这是刻意的：行只在渲染期存在，调用方若传元素就得在挂载后才构造闭包，很容易传成
   * `null`；那会让分档静默退化成恒定的 `'above'`，同时废掉「拖进模块 / 集合」与向下
   * 重排，且任何地方都不报错。从事件里读就没有可忘记的参数。
   */
  const computeZoneFromEvent = (
    e: ReactDragEvent,
    drag: DragPayload | null,
    target: { kind: TargetKind; id: string },
  ): DropZone =>
    zoneFromRowPosition(
      e.currentTarget.getBoundingClientRect(),
      e.clientY,
      drag ? canNestInto(drag.kind, target.kind) : false,
    );

  const makeDragOverHandler = (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'move';
    const raw = e.dataTransfer.getData(DND_MIME);
    // 浏览器在 `dragover` 期间不交回拖拽数据 —— 规范只允许 `drop` 和 `dragstart` 读它 ——
    // 故每次悬停 `decodeDrag` 都是 null，`canNestInto` 永远不会被问到。那足以隐藏容器行的
    // 中间一带，而那正是唯一在告诉用户「这一行会把节点收作子节点、而非在其旁重排」的信号。
    // dragstart 时 `setDataTransfer` 写入的镜像让这个提示说真话。
    const drag = decodeDrag(raw) ?? lastDragPayload;
    const zone = computeZoneFromEvent(e, drag, target);
    const hint = { key: `${target.kind}:${target.id}`, zone };
    setDropHint((prev) => (prev && prev.key === hint.key && prev.zone === hint.zone ? prev : hint));
  };

  const makeDragLeaveHandler =
    (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => {
      // 只在真正离开这一行时才清提示（移过子元素不算）。relatedTarget 告诉我们指针去了哪；
      // 仍在自身子树内就忽略。
      const next = e.relatedTarget as Node | null;
      if (next && e.currentTarget.contains(next)) return;
      setDropHint((prev) => (prev && prev.key === `${target.kind}:${target.id}` ? null : prev));
    };

  const handleDragEnd = () => {
    setDropHint(null);
    lastDragPayload = null;
  };

  return {
    dropHint,
    setDataTransfer,
    makeDropHandler,
    makeDragOverHandler,
    makeDragLeaveHandler,
    handleDragEnd,
  };
}
