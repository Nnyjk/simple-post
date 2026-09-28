import { useState, type DragEvent as ReactDragEvent } from 'react';
import type { Collection, Endpoint, Module } from '@/types/domain';
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

export interface TreeDndBindings {
  modules: Module[];
  collections: Collection[];
  endpoints: Endpoint[];
  activeProjectId: string | null;
  moveModule: (moduleId: string, targetIndex: number) => void;
  moveCollections: (
    collectionIds: string[],
    target: {
      parentKind: 'module' | 'collection';
      parentId: string;
      index: number;
    },
  ) => void;
  moveEndpoints: (endpointIds: string[], target: { collectionId: string; index: number }) => void;
}

/**
 * All drag-and-drop plumbing for the tree: payload encoding, the
 * cross-project guard, drop-index computation, the above/into/below
 * zone resolution and the live drop hint. State and store values are
 * passed in by `ProjectTree` — this hook owns no store selectors.
 */
export function useTreeDnd({
  modules,
  collections,
  endpoints,
  activeProjectId,
  moveModule,
  moveCollections,
  moveEndpoints,
}: TreeDndBindings) {
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
      if (kind === 'module') return modules.find((m) => m.id === id)?.projectId;
      if (kind === 'collection') {
        const c = collections.find((cc) => cc.id === id);
        if (!c) return undefined;
        return modules.find((m) => m.id === c.moduleId)?.projectId;
      }
      // endpoint
      const e = endpoints.find((ep) => ep.id === id);
      if (!e) return undefined;
      const c = collections.find((cc) => cc.id === e.collectionId);
      if (!c) return undefined;
      return modules.find((m) => m.id === c.moduleId)?.projectId;
    };
    const targetProject = projectIdOf(target.kind, target.id);
    if (!targetProject) return false;
    for (const id of drag.ids) {
      if (projectIdOf(drag.kind, id) !== targetProject) return false;
    }
    return true;
  };

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
      // Insert at the end of the destination's children list.
      if (target.kind === 'module') {
        const childCount = collections.filter(
          (c) => c.moduleId === target.id && c.parentCollectionId === null,
        ).length;
        return childCount;
      }
      if (target.kind === 'collection') {
        const childCount = collections.filter((c) => c.parentCollectionId === target.id).length;
        return childCount;
      }
      return null;
    }
    // above / below: insert before/after the target row.
    if (target.kind === 'module') {
      const modList = modules
        .filter((m) => m.projectId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = modList.findIndex((m) => m.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    if (target.kind === 'collection') {
      const col = collections.find((c) => c.id === target.id);
      if (!col) return null;
      const siblings = collections
        .filter(
          (c) => c.moduleId === col.moduleId && c.parentCollectionId === col.parentCollectionId,
        )
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = siblings.findIndex((c) => c.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    // endpoint: insert before/after in the same collection.
    if (target.kind === 'endpoint') {
      const ep = endpoints.find((e) => e.id === target.id);
      if (!ep) return null;
      const siblings = endpoints
        .filter((e) => e.collectionId === ep.collectionId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const idx = siblings.findIndex((e) => e.id === target.id);
      if (idx < 0) return null;
      return zone === 'above' ? idx : idx + 1;
    }
    return null;
  };

  /**
   * Cycle guard for collection-on-collection drops: the target must
   * not be the source itself or any descendant (we'd create a loop).
   */
  const wouldCycleCollectionInto = (
    collectionIds: string[],
    target: { kind: 'module' | 'collection'; parentId: string },
  ): boolean => {
    if (target.kind !== 'collection') return false;
    const childIndex = new Map<string, string[]>();
    for (const c of collections) {
      if (c.parentCollectionId) {
        const bucket = childIndex.get(c.parentCollectionId);
        if (bucket) bucket.push(c.id);
        else childIndex.set(c.parentCollectionId, [c.id]);
      }
    }
    const blocked = new Set<string>(collectionIds);
    const queue = [...collectionIds];
    while (queue.length > 0) {
      const id = queue.shift()!;
      const kids = childIndex.get(id) ?? [];
      for (const k of kids) {
        if (!blocked.has(k)) {
          blocked.add(k);
          queue.push(k);
        }
      }
    }
    return blocked.has(target.parentId);
  };

  /**
   * Drop handler factory. Bound per-row so each row can decide whether
   * to accept the drop (and which index to insert at).
   */
  const makeDropHandler =
    (target: { kind: TargetKind; id: string }, targetEl: HTMLElement | null) =>
    (e: ReactDragEvent) => {
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

      // Compute drop zone from the current cursor position (re-read
      // from the event; the row passed the zone via dropHint, but the
      // drop target may have moved since the last dragover).
      const zone = computeZoneFromEvent(e, drag, target, targetEl);
      const index = computeTargetIndex(drag, target, zone);
      if (index === null) return;

      if (drag.kind === 'module') {
        if (target.kind !== 'module') return;
        // Disallow dropping a module onto itself (already filtered above).
        moveModule(drag.ids[0], index);
        return;
      }
      if (drag.kind === 'collection') {
        if (target.kind === 'module') {
          moveCollections(drag.ids, {
            parentKind: 'module',
            parentId: target.id,
            index,
          });
          return;
        }
        if (target.kind === 'collection') {
          if (zone === 'into') {
            if (wouldCycleCollectionInto(drag.ids, { kind: 'collection', parentId: target.id })) {
              return;
            }
            moveCollections(drag.ids, {
              parentKind: 'collection',
              parentId: target.id,
              index,
            });
            return;
          }
          // above/below: drop as sibling (preserving the target's
          // parent). This handles the "脱离集合" gesture: a sub-
          // collection can be dragged next to its parent and dropped
          // as a sibling.
          const targetCol = collections.find((c) => c.id === target.id);
          if (!targetCol) return;
          if (targetCol.parentCollectionId === null) {
            moveCollections(drag.ids, {
              parentKind: 'module',
              parentId: targetCol.moduleId,
              index,
            });
          } else {
            moveCollections(drag.ids, {
              parentKind: 'collection',
              parentId: targetCol.parentCollectionId,
              index,
            });
          }
          return;
        }
        return;
      }
      // drag.kind === 'endpoint'
      if (target.kind === 'collection') {
        if (zone === 'into' || zone === 'below') {
          // Dropping into / just below a collection row both map to
          // "append into that collection". For 'into' it's natural;
          // for 'below' it's a forgiving fallback so the user doesn't
          // have to perfectly hit the middle of the row.
          const childCount = endpoints.filter((e2) => e2.collectionId === target.id).length;
          moveEndpoints(drag.ids, {
            collectionId: target.id,
            index: childCount,
          });
          return;
        }
        // 'above' on a collection row: drop as last item in the
        // collection that comes BEFORE the target in the tree
        // (typically the previous sibling at the same level). To keep
        // the UX simple, just append to the target's collection — the
        // caller can re-drag to reorder.
        moveEndpoints(drag.ids, {
          collectionId: target.id,
          index: 0,
        });
        return;
      }
      if (target.kind === 'endpoint') {
        const ep = endpoints.find((e2) => e2.id === target.id);
        if (!ep) return;
        moveEndpoints(drag.ids, {
          collectionId: ep.collectionId,
          index,
        });
      }
    };

  /**
   * Whether the dragged kind can be **nested into** the given target
   * kind. Drives the 25/50/25 (vs 50/50) split on container rows.
   *
   * Rules:
   *   - Modules can never be embedded (per the user's "模块无法被嵌
   *     入集合" rule), so `into` is never valid when target is module.
   *   - Collections can be embedded under another collection.
   *   - Endpoints can be embedded under a collection.
   */
  const canNestInto = (dragKind: TargetKind, targetKind: TargetKind): boolean => {
    if (targetKind === 'module') return dragKind === 'collection';
    if (targetKind === 'collection') return dragKind === 'collection' || dragKind === 'endpoint';
    return false;
  };

  /**
   * Compute the DropZone from a DragEvent, based on cursor Y relative
   * to the row. When the dragged kind can be nested into the target
   * (see `canNestInto`), containers use a 25/50/25 split; otherwise
   * (or for leaves like endpoints) the row uses a plain 50/50 split.
   */
  const computeZoneFromEvent = (
    e: ReactDragEvent,
    drag: DragPayload | null,
    target: { kind: TargetKind; id: string },
    el: HTMLElement | null,
  ): DropZone => {
    const rect = el?.getBoundingClientRect();
    if (!rect) return 'above';
    const y = e.clientY - rect.top;
    const ratio = y / rect.height;
    const supportsInto = drag ? canNestInto(drag.kind, target.kind) : false;
    if (!supportsInto) {
      return ratio < 0.5 ? 'above' : 'below';
    }
    if (ratio < 0.28) return 'above';
    if (ratio > 0.72) return 'below';
    return 'into';
  };

  const makeDragOverHandler =
    (target: { kind: TargetKind; id: string }, el: HTMLElement | null) => (e: ReactDragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      e.dataTransfer.dropEffect = 'move';
      const raw = e.dataTransfer.getData(DND_MIME);
      const drag = decodeDrag(raw);
      const zone = computeZoneFromEvent(e, drag, target, el);
      const hint = { key: `${target.kind}:${target.id}`, zone };
      setDropHint((prev) =>
        prev && prev.key === hint.key && prev.zone === hint.zone ? prev : hint,
      );
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
