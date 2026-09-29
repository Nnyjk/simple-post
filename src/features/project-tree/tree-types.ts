import type { DragEvent as ReactDragEvent } from 'react';
import type { EndpointDetail, ResourceNode } from '@/types/domain';

export type TargetKind = 'project' | 'module' | 'collection' | 'endpoint';

export interface DeleteTarget {
  kind: TargetKind;
  id: string;
  name: string;
  /** Human-readable cascade description, shown under the confirm message. */
  cascade?: string;
}

/** The row currently being renamed inline, if any. */
export type EditTarget = { kind: TargetKind; id: string };

export interface DragPayload {
  kind: TargetKind;
  /** All ids being dragged (single-select = array of one). */
  ids: string[];
}

/**
 * What part of a target row the cursor is currently over. Drives both
 * the visual indicator and the resulting move call:
 *
 *   - `above`  → insert as previous sibling (or previous item in a flat list)
 *   - `into`   → only valid for containers (module / collection); nest under it
 *   - `below`  → insert as next sibling
 *
 * Endpoints are leaves — they only support `above` / `below`.
 */
export type DropZone = 'above' | 'into' | 'below';

export type CollectionBranch = {
  collection: ResourceNode;
  endpoints: ResourceNode[];
  /**
   * Each endpoint paired with its configuration. The tree draws a
   * method pill and a url, both of which live in the detail row, so
   * carrying the pair here saves `rows/EndpointRow` a store lookup per
   * rendered endpoint.
   */
  endpointDetails: Record<string, EndpointDetail>;
  children: CollectionBranch[];
};

export type TreeBranch = { module: ResourceNode; collections: CollectionBranch[] };

/**
 * Everything the row components need in order to wire drag-and-drop.
 * Built once by `ProjectTree` out of the `useTreeDnd` helpers and
 * threaded down to `CollectionBranch` as a plain prop (no React
 * context), so `rows/*` and `hooks/*` never import each other.
 */
export interface DragContextValue {
  /**
   * One move for the whole tree. `parentId: null` means "top of the
   * project" and is legal only for a module — the destination parent's
   * own `type` is what decides where anything else may land, which is
   * why there is no `parentKind` to pass.
   */
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => void;
  setDataTransfer: (e: ReactDragEvent, payload: DragPayload) => void;
  makeDragOverHandler: (
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ) => (e: ReactDragEvent) => void;
  makeDragLeaveHandler: (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => void;
  makeDropHandler: (
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ) => (e: ReactDragEvent) => void;
  handleDragEnd: (e: ReactDragEvent) => void;
  dropHint: { key: string; zone: DropZone } | null;
}
