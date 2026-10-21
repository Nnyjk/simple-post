import type { DragEvent as ReactDragEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';

export type TargetKind = 'project' | 'module' | 'collection' | 'endpoint';

export interface DeleteTarget {
  kind: TargetKind;
  id: string;
  name: string;
  /** 级联影响的可读描述，显示在确认文案下方。 */
  cascade?: string;
}

/** 正在就地重命名的行，没有则为 undefined。 */
export type EditTarget = { kind: TargetKind; id: string };

export interface DragPayload {
  kind: TargetKind;
  /** 被拖的全部 id（单选就是一个元素的数组）。 */
  ids: string[];
}

/**
 * 光标当前停在目标行的哪一块，同时驱动视觉指示与最终的 move 调用：
 *
 *   - `above` → 作为上一个兄弟插入（打平的列表里则是上一项）
 *   - `into`  → 仅对容器（模块 / 集合）有效，嵌到它下面
 *   - `below` → 作为下一个兄弟插入
 *
 * 接口是叶子，只支持 `above` / `below`。
 */
export type DropZone = 'above' | 'into' | 'below';

export type CollectionBranch = {
  collection: ResourceNode;
  endpoints: ResourceNode[];
  /**
   * 每个接口配一份它的请求配置。树要画 method 药丸和 url，两者都在 detail 行里，
   * 这里带上配对，省得 `rows/EndpointRow` 每渲染一个接口就查一次 store。
   */
  endpointDetails: Record<string, EndpointDetail>;
  children: CollectionBranch[];
};

export type TreeBranch = {
  module: ResourceNode;
  /**
   * 直接挂在模块下、没有自己集合的接口。模块两类子节点都收，故它们与集合并列渲染，
   * 而非塞进某个集合 —— `TreeBranch` 单独带着它们，正如 `CollectionBranch` 带着自己那份。
   */
  endpoints: ResourceNode[];
  /** store 的 detail map 中覆盖 `endpoints` 的那一部分。 */
  endpointDetails: Record<string, EndpointDetail>;
  collections: CollectionBranch[];
};

/**
 * 行组件拖拽所需的全部数据。由 `ProjectTree` 用 `useTreeDnd` 的 helper 建一次，
 * 再作为普通 prop 往下传给 `CollectionBranch`（不走 React context），这样 `rows/*` 与
 * `hooks/*` 之间不会互相 import。
 */
export interface DragContextValue {
  /**
   * 整棵树一次 move。`parentId: null` 表示「项目顶层」，且只有模块合法；
   * 别的节点能落在哪，由目标父节点自己的 `type` 决定，故没有 `parentKind` 要传。
   */
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => void;
  setDataTransfer: (e: ReactDragEvent, payload: DragPayload) => void;
  makeDragOverHandler: (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => void;
  makeDragLeaveHandler: (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => void;
  makeDropHandler: (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => void;
  handleDragEnd: (e: ReactDragEvent) => void;
  dropHint: { key: string; zone: DropZone } | null;
}
