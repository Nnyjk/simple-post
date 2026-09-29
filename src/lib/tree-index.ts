import type { ResourceNode } from '@/types/domain';

/**
 * The `parentId -> children` index the resource tree is built on.
 *
 * Every node names its own parent, so the whole tree — modules,
 * collections, endpoints — is one `parentId → children` map rather
 * than three tables that each need their own join. This module is the
 * single place that builds it: the store needs it to walk subtrees and
 * to find a move's destination siblings, and the renderer's `buildTree`
 * needs the same thing narrowed to one project. Two copies of this
 * loop were the reason it had to be merged.
 *
 * Two invariants the callers depend on:
 *
 *   * **Purity.** A fresh map of fresh arrays, holding the caller's node
 *     objects by reference. Nothing the caller owns is reordered or
 *     written to. That is what makes it safe to build inside a
 *     `set()` callback.
 *   * **Not a selector.** The returned map is a new object every call,
 *     and Zustand v5 compares with `Object.is` — a selector returning
 *     one of these re-renders forever. Build it in a `useMemo` or in an
 *     event handler.
 *
 * `sortOrder` is scoped to the *parent*, so the sort is per bucket; the
 * same number may mean different positions under two different parents
 * and both have to survive.
 */
export function indexByParent(
  nodes: readonly ResourceNode[],
  /**
   * Narrow to one project's tree. Omitted means the whole workspace —
   * the store's case, where a cascade delete or a move must see every
   * project's nodes at once, not just the one on screen.
   */
  rootId?: string,
): Map<string | null, ResourceNode[]> {
  const byParent = new Map<string | null, ResourceNode[]>();
  for (const n of nodes) {
    if (rootId !== undefined && n.rootId !== rootId) continue;
    const bucket = byParent.get(n.parentId);
    if (bucket) bucket.push(n);
    else byParent.set(n.parentId, [n]);
  }
  for (const bucket of byParent.values()) {
    bucket.sort((a, b) => a.sortOrder - b.sortOrder);
  }
  return byParent;
}
