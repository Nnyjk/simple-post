import { useState, type DragEvent as ReactDragEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { DragPayload, DropZone, TreeNodeKind } from '../tree-types';
import { DND_MIME, decodeDrag, encodeDrag } from '../tree-utils';
import { resolveDrop, zoneFor } from '../drop-resolve';

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

export interface TreeDndBindings {
  /** The active project's nodes — already filtered by `rootId`. */
  nodes: ResourceNode[];
  activeProjectId: string | null;
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => void;
}

/**
 * 树的全部拖放管线：载荷编码、落点判定、放置提示。
 *
 * 落点规则本身不在这一层 —— 它住在 `../drop-resolve.ts`，是一组纯函数，规则全部来自
 * 父子类型矩阵（`@/lib/node-placement.ts`）。这一层只负责把浏览器事件翻译成那些函数
 * 的入参，以及把「这一行接受哪些放置区」的结论画成提示。
 *
 * 状态与 store 值由 `ProjectTree` 传入 —— 本 hook 不持有任何 store selector。
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
   * drop 处理器工厂。按行绑定，好让每一行自己决定是否接受放置（以及插到哪个索引）。
   *
   * 落点由 `resolveDrop` 算出：它给出新父级与新槽位，或者给出被拒绝的理由。被拒时
   * 什么也不做 —— 界面在 `dragover` 阶段就没有画提示，光标也已经是「不可放」。
   */
  const makeDropHandler = (target: { kind: TreeNodeKind; id: string }) => (e: ReactDragEvent) => {
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

    const decision = resolveDrop({
      drag,
      target,
      rect: e.currentTarget.getBoundingClientRect(),
      clientY: e.clientY,
      nodes,
      activeProjectId,
    });
    if (!decision.ok) return;
    moveNodes(drag.ids, decision.parentId, decision.index);
  };

  /**
   * 悬停提示：这一行在当前拖拽下接受哪一块。
   *
   * 浏览器在 `dragover` 期间不交回拖拽数据 —— 规范只允许 `drop` 和 `dragstart` 读它 ——
   * 故每次悬停 `decodeDrag` 都是 null，判定「能不能嵌进去」永远不会被问到。那足以隐藏容器行
   * 的中间一带，而那正是唯一在告诉用户「这一行会把节点收作子节点、而非在其旁重排」的信号。
   * dragstart 时 `setDataTransfer` 写入的镜像让这个提示说真话。
   */
  const makeDragOverHandler =
    (target: { kind: TreeNodeKind; id: string }) => (e: ReactDragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const raw = e.dataTransfer.getData(DND_MIME);
      const drag = decodeDrag(raw) ?? lastDragPayload;
      const key = `${target.kind}:${target.id}`;
      const zone = drag
        ? zoneFor(drag, target, nodes, e.currentTarget.getBoundingClientRect(), e.clientY)
        : null;
      if (!zone) {
        // 这一行不接受这次拖拽 —— 光标显示「不可放」，也不画落点提示。
        e.dataTransfer.dropEffect = 'none';
        setDropHint((prev) => (prev && prev.key === key ? null : prev));
        return;
      }
      e.dataTransfer.dropEffect = 'move';
      const hint = { key, zone };
      setDropHint((prev) =>
        prev && prev.key === hint.key && prev.zone === hint.zone ? prev : hint,
      );
    };

  const makeDragLeaveHandler =
    (target: { kind: TreeNodeKind; id: string }) => (e: ReactDragEvent) => {
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
