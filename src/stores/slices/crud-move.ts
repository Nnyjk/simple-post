/**
 * CRUD sub-factory — drag-and-drop moves.
 *
 * One of five sub-factories composed by `createCrudSlice` (see
 * `../crud-slice.ts` for the slice-wide contract: optimistic `set()` →
 * `invokeCmd` → snapshot-and-rollback).
 *
 * Moving is one action, not three, and it cannot be expressed as a plain
 * field write: the local `set()` re-derives `sortOrder` across the
 * destination siblings and drags every descendant's `rootId` along with
 * the subtree it moves. The `ResourceNodeType::may_under` contract —
 * module → top level only, endpoint → under a collection, collection →
 * module or collection — is enforced in Rust by a trigger; the guards
 * below are the optimistic mirror of it, so a rejected drop never
 * renders state the backend would have refused.
 */
import type { StateCreator } from 'zustand';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { indexByParent } from '@/lib/tree-index';
import type { AppState } from '../app-store';
import { collectSubtree, renumber } from './crud-helpers';

/** Move action owned by the CRUD slice. */
export interface MoveCrudActions {
  /**
   * Move one or many nodes under `parentId` at sibling slot `index`.
   *
   * `parentId: null` means "top of the project" and is legal only for a
   * module. `nodeIds` is in the order the nodes should land, so a
   * multi-select drag keeps the user's pick order.
   */
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => Promise<void>;
}

export const createMoveCrud: StateCreator<AppState, [], [], MoveCrudActions> = (set, get) => ({
  moveNodes: async (nodeIds, parentId, index) => {
    const prevNodes = get().nodes;
    set((s) => {
      if (nodeIds.length === 0) return {};
      const idSet = new Set(nodeIds);
      const childrenIndex = indexByParent(s.nodes);
      const sources = s.nodes.filter((n) => idSet.has(n.id));
      if (sources.length === 0) return {};

      // The destination parent, and the project it puts the nodes in.
      // `rootId` rides on every node, so the project-boundary check is a
      // string compare instead of the old walk up to the module.
      let targetRootId: string;
      if (parentId === null) {
        // "Top of the project" — legal only for a module.
        if (sources.some((n) => n.kind !== 'module')) return {};
        targetRootId = sources[0].rootId;
      } else {
        const targetParent = s.nodes.find((n) => n.id === parentId);
        if (!targetParent) return {};
        targetRootId = targetParent.rootId;
      }

      // Cycle guard: a node cannot be moved under itself or any
      // descendant. The UI disables those drop targets, but a stale
      // drag payload can still name one.
      const movedSubtrees = new Set<string>();
      for (const src of sources) {
        for (const nid of collectSubtree(src.id, childrenIndex)) movedSubtrees.add(nid);
      }
      if (parentId !== null && movedSubtrees.has(parentId)) return {};

      // Every source must already belong to the destination's project.
      // The server refuses the move otherwise; refusing locally keeps
      // the optimistic state from showing something that will 400.
      for (const src of sources) {
        if (src.rootId !== targetRootId) return {};
      }

      // Destination siblings with the movers removed — they are
      // reinserted at `index` below.
      //
      // For a top-level drop the siblings are the destination project's
      // modules, which are the nodes with no parent; scoping by
      // `parentId === null` alone would sweep in every project's
      // top level at once.
      const siblings =
        parentId === null
          ? s.nodes
              .filter((n) => n.parentId === null && n.rootId === targetRootId && !idSet.has(n.id))
              .sort((a, b) => a.sortOrder - b.sortOrder)
          : (childrenIndex.get(parentId) ?? []).filter((n) => !idSet.has(n.id));

      // The moved rows keep the caller's order, so a multi-select drag
      // lands the way the user picked them.
      const movedRows = sources
        .slice()
        .sort((a, b) => nodeIds.indexOf(a.id) - nodeIds.indexOf(b.id))
        .map((n) => ({ ...n, parentId, rootId: targetRootId }));

      const clampedIndex = Math.max(0, Math.min(index, siblings.length));
      const renumbered = renumber([
        ...siblings.slice(0, clampedIndex),
        ...movedRows,
        ...siblings.slice(clampedIndex),
      ]);
      const renumberedById = new Map(renumbered.map((n) => [n.id, n] as const));

      // Every node either keeps its slot (outside the destination), is
      // one of the renumbered rows, or is a descendant of a moved row
      // whose `rootId` has to follow. The last case is the one that used
      // to drift: the old store fixed a collection's `moduleId` here
      // while the database left the descendants alone.
      const next = s.nodes.map((n) => {
        const renamed = renumberedById.get(n.id);
        if (renamed) return renamed;
        if (movedSubtrees.has(n.id) && n.rootId !== targetRootId) {
          return { ...n, rootId: targetRootId };
        }
        return n;
      });

      return { nodes: next };
    });
    try {
      await invokeCmd<void>(COMMANDS.nodesMove, { nodeIds, parentId, index });
    } catch (e) {
      console.error('[store] nodes_move failed', e);
      set({ nodes: prevNodes });
    }
  },
});
