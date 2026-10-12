import { useState, type DragEvent as ReactDragEvent } from 'react';
import type { ResourceNode } from '@/types/domain';
import type { DragPayload, DropZone, TargetKind } from '../tree-types';
import { DND_MIME, decodeDrag, encodeDrag } from '../tree-utils';

/**
 * Module-level scratchpad for the in-flight drag. Some synthetic
 * event drivers (notably Playwright's `dragAndDrop`) fire the HTML5
 * dragstart / dragover / drop sequence but don't propagate the
 * `DataTransfer` payload that `setData` writes during dragstart. We
 * mirror the payload here so `makeDropHandler` can still recover the
 * drag context on drop. Real browsers populate dataTransfer normally
 * — this is purely a fallback for environments that drop it.
 */
let lastDragPayload: DragPayload | null = null;

/**
 * Set the in-app drag payload on the native dataTransfer. Also sets
 * a `text/plain` fallback because some browsers refuse to start a
 * drag without ANY text payload, and our custom mime alone isn't
 * always enough.
 */
function setDataTransfer(e: ReactDragEvent, payload: DragPayload) {
  const json = encodeDrag(payload);
  e.dataTransfer.setData(DND_MIME, json);
  e.dataTransfer.setData('text/plain', json);
  e.dataTransfer.effectAllowed = 'move';
  lastDragPayload = payload;
}

/**
 * The zone math, as a pure function of (row rect, cursor, whether the
 * dragged kind may nest here).
 *
 * Split out from `computeZoneFromEvent` so the 25/50/25 vs 50/50 rule is
 * testable without a DOM — this is the one piece of the drop decision
 * that can silently collapse without anyone noticing (see the note in
 * `computeZoneFromEvent` about where the rect comes from).
 *
 * `rect` is the row's own client rect; `supportsInto` is
 * `canNestInto(dragKind, targetKind)`. Without it — a leaf row, or a
 * drag whose kind may not nest here — the row keeps a plain 50/50
 * split and can never answer `into`.
 *
 * A missing or zero-height rect has no ratio to speak of, so it
 * resolves to `above`: the one zone that is meaningful in every
 * context, and the only one that can't nest a node somewhere invalid.
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
 * All drag-and-drop plumbing for the tree: payload encoding, the
 * cross-project guard, drop-index computation, the above/into/below
 * zone resolution and the live drop hint. State and store values are
 * passed in by `ProjectTree` — this hook owns no store selectors.
 *
 * One node array and one move action replaced three of each. The
 * per-kind branches that used to pick a destination are gone: where a
 * node lands is decided by the target's `parentId` plus the type matrix
 * the server enforces.
 */
export function useTreeDnd({ nodes, activeProjectId, moveNodes }: TreeDndBindings) {
  /**
   * Live indicator of the current drop target while a drag is in
   * flight. `null` when nothing is being dragged. Keyed by
   * `${kind}:${id}` so multiple targets can highlight at once (e.g.
   * an `into` highlight on the parent + a line on the target row).
   */
  const [dropHint, setDropHint] = useState<{
    key: string;
    zone: DropZone;
  } | null>(null);

  /**
   * Resolve the `(source, target)` pair of project ids for a potential
   * drop. Returns `null` if the drop would cross project boundaries
   * (rule: "任何拖拽都不能使目标脱离项目").
   */
  const sameProjectGuard = (
    drag: DragPayload,
    target: { kind: TargetKind; id: string },
  ): boolean => {
    const projectIdOf = (kind: TargetKind, id: string): string | undefined => {
      if (kind === 'project') return id;
      // Every other kind is a node, and every node records its own
      // owner — so the whole ladder is one lookup where it used to be
      // endpoint → collection → module → project.
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
   * Siblings of `parentId` within this project, in display order.
   *
   * `parentId: null` means the project's top level — the modules. That
   * is scoped by `rootId` too, because `null` is every project's top
   * level at once.
   */
  const siblingsOf = (parentId: string | null): ResourceNode[] =>
    nodes
      .filter((n) => n.parentId === parentId && (parentId !== null || n.rootId === activeProjectId))
      .sort((a, b) => a.sortOrder - b.sortOrder);

  /**
   * Compute the index at which the dragged items should be inserted
   * inside the new parent. Walks the visible tree to find the right
   * sibling, since `index` in our move actions refers to position
   * inside the destination's sorted children list.
   */
  const computeTargetIndex = (
    drag: DragPayload,
    target: { kind: TargetKind; id: string },
    zone: DropZone,
  ): number | null => {
    if (zone === 'into') {
      // Insert at the end of the destination's children list. A module
      // holds collections and endpoints; a collection holds endpoints and
      // further collections — one count either way, because the move
      // action re-numbers only the rows it inserts among.
      if (target.kind === 'module' || target.kind === 'collection') {
        return siblingsOf(target.id).length;
      }
      return null;
    }
    // above / below: insert before/after the target row, among the
    // target's own siblings.
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
   * Cycle guard: the target must not be the source itself or any
   * descendant, or we'd nest a node inside itself.
   *
   * Walks `parentId` rather than special-casing collections: a module
   * dragged onto a collection inside it is the same loop as a
   * collection dragged onto its own child, and the old guard only
   * caught the second.
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
   * Drop handler factory. Bound per-row so each row can decide whether
   * to accept the drop (and which index to insert at).
   */
  const makeDropHandler = (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const raw = e.dataTransfer.getData(DND_MIME) || e.dataTransfer.getData('text/plain');
    let drag = decodeDrag(raw);
    // Fallback: some synthetic drag drivers (Playwright) don't
    // propagate dataTransfer across dragstart → drop. The
    // `lastDragPayload` mirror written in `setDataTransfer` covers
    // that case.
    if (!drag) drag = lastDragPayload;
    setDropHint(null);
    lastDragPayload = null;
    if (!drag) return;
    if (drag.ids.includes(target.id) && drag.kind === target.kind) return;
    if (!sameProjectGuard(drag, target)) return;

    // A module is the top of a project; it has no node parent, so it
    // can only be reordered among its siblings.
    if (drag.kind === 'module') {
      if (target.kind !== 'module') return;
      const idx = computeTargetIndex(drag, target, computeZoneFromEvent(e, drag, target));
      if (idx === null) return;
      moveNodes(drag.ids, null, idx);
      return;
    }

    if (target.kind === 'module') {
      // A module takes collections *and* endpoints — an endpoint that
      // needs no collection of its own lands straight on it. Anything
      // else (another module) the server's type matrix refuses, so
      // refusing here keeps the optimistic state from showing a move
      // that 400s.
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
      // above/below: drop as a sibling of the target, keeping the
      // target's parent. This is the "脱离集合" gesture — a nested
      // collection dragged next to its parent lands beside it.
      //
      // `parentId` covers both cases with no branch: a top-level
      // collection's parent *is* its module node, and a nested one's
      // is the collection above it. That is the whole point of the
      // unified parent column.
      const targetNode = nodes.find((n) => n.id === target.id);
      if (!targetNode || !targetNode.parentId) return;
      if (drag.kind === 'collection' && wouldCycleInto(drag.ids, target.id)) return;
      const idx = computeTargetIndex(drag, target, zone);
      if (idx === null) return;
      moveNodes(drag.ids, targetNode.parentId, idx);
      return;
    }

    // target.kind === 'endpoint' — dropping beside an endpoint. Only
    // an endpoint goes here: the move makes the dragged row a sibling
    // of the target, and the gesture this branch implements ("reorder
    // among a collection's endpoints") has no meaning for a container.
    // A collection whose sibling slot is a module-level endpoint is
    // reachable by dropping onto the module row instead.
    if (drag.kind !== 'endpoint') return;
    const targetNode = nodes.find((n) => n.id === target.id);
    if (!targetNode || !targetNode.parentId) return;
    const idx = computeTargetIndex(drag, target, computeZoneFromEvent(e, drag, target));
    if (idx === null) return;
    moveNodes(drag.ids, targetNode.parentId, idx);
  };

  /**
   * Whether the dragged kind can be **nested into** the given target
   * kind. Drives the 25/50/25 (vs 50/50) split on container rows.
   *
   * Rules — the mirror of `ResourceNodeType::may_under`:
   *   - Modules can never be embedded (per the user's "模块无法被嵌
   *     入集合" rule), so `into` is never valid when the dragged kind
   *     is a module.
   *   - Collections can be embedded under a module or another
   *     collection.
   *   - Endpoints can be embedded under a module or a collection.
   */
  const canNestInto = (dragKind: TargetKind, targetKind: TargetKind): boolean => {
    if (targetKind === 'module') return dragKind === 'collection' || dragKind === 'endpoint';
    if (targetKind === 'collection') return dragKind === 'collection' || dragKind === 'endpoint';
    return false;
  };

  /**
   * Compute the DropZone from a DragEvent, based on cursor Y relative
   * to the row. When the dragged kind can be nested into the target
   * (see `canNestInto`), containers use a 25/50/25 split; otherwise
   * (or for leaves like endpoints) the row uses a plain 50/50 split.
   *
   * The rect is read off `e.currentTarget` — the row element the
   * handler is bound to — rather than threaded in as an argument. That
   * is deliberate: the row only exists at render time, so a caller
   * passing an element would have had to build the closure after mount
   * and could easily pass `null` instead. When that happened the split
   * silently degraded to a constant `'above'`, which took out both
   * "drop into a module / collection" and downward reordering with no
   * error anywhere. Reading it from the event means there is no
   * argument to forget.
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
    // Browsers refuse to hand back drag data during `dragover` — the
    // spec only lets `drop` and `dragstart` read it — so `decodeDrag`
    // is null on every hover and `canNestInto` would never be consulted.
    // That is enough to hide the middle band of a container row, which
    // is the only thing telling the user "this row will take the node
    // as a child rather than reorder beside it". The mirror written by
    // `setDataTransfer` at dragstart is what makes the hint honest.
    const drag = decodeDrag(raw) ?? lastDragPayload;
    const zone = computeZoneFromEvent(e, drag, target);
    const hint = { key: `${target.kind}:${target.id}`, zone };
    setDropHint((prev) => (prev && prev.key === hint.key && prev.zone === hint.zone ? prev : hint));
  };

  const makeDragLeaveHandler =
    (target: { kind: TargetKind; id: string }) => (e: ReactDragEvent) => {
      // Only clear the hint when we actually leave the row (not when
      // moving over a child element). relatedTarget tells us where the
      // pointer went; if it's still inside our subtree, ignore.
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
