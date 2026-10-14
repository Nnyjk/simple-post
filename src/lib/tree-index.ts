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

/**
 * The module an endpoint ultimately sits under, or `null` when it is not
 * under one.
 *
 * A project's own id is denormalised onto every node as `rootId`, which
 * makes "which project" one lookup — but there is no equivalent column
 * for the module, so this walks `parentId` instead. That asymmetry is
 * deliberate on the DB side (the module changes on a move, the project
 * never does) and cheap here: the walk is bounded by collection nesting
 * depth, and `nodes` is already in memory.
 *
 * Needed because baseUrl **values** are per module: an endpoint resolves
 * its service against `env.moduleBaseUrls[defId][moduleId]` (see
 * `resolveEndpointBaseUrl` in `lib/url.ts`), and the resolution layer has
 * to know which module the endpoint belongs to before it can read that
 * cell. The service *list* is project-level and needs no such narrowing —
 * that asymmetry is why this function stays even though services no
 * longer belong to a module.
 *
 * Takes the whole node list and does its own index lookup rather than a
 * prebuilt map, so callers can pass `store.nodes` directly. It is
 * **not** memoised: call it inside a `useMemo`, never as a Zustand
 * selector (a fresh string is stable under `Object.is`, so a selector
 * returning this is safe, but the walk itself should not rerun per
 * render).
 *
 * DRAFTS ARE NOT IN `nodes`
 * -------------------------
 * An unsaved endpoint lives only in `draftEndpoints` until Save, so
 * `byId.get(draftId)` misses and the walk returned `null` — which reads
 * downstream as "this endpoint has no module", i.e. no baseUrl cell at
 * all. The parents *are* real rows, so the walk only ever lacked a
 * starting point: pass the draft's own node as `self`.
 */
export function moduleIdOfEndpoint(
  nodes: readonly ResourceNode[],
  endpointId: string | null | undefined,
  /**
   * The endpoint's own row when `nodes` does not carry it — an unsaved
   * draft, which `useActiveEndpointResource` synthesises from the
   * `DraftEndpoint`. Ignored when `nodes` has a row for `endpointId`, so a
   * caller that always passes `resource.node` is right for both storage
   * forms.
   */
  self?: ResourceNode | null,
): string | null {
  if (!endpointId) return null;
  const byId = new Map<string, ResourceNode>();
  for (const n of nodes) byId.set(n.id, n);

  // `kind` is the node's own type; an endpoint may sit directly under a
  // module (no collection), so both shapes terminate here.
  let cur = byId.get(endpointId) ?? (self?.id === endpointId ? self : undefined);
  let guard = 0;
  while (cur && guard++ < 64) {
    if (cur.kind === 'module') return cur.id;
    if (!cur.parentId) return null;
    cur = byId.get(cur.parentId);
  }
  // A cycle cannot happen — the schema's parent matrix and the move
  // guard both forbid it — but a bounded walk that silently returns
  // `null` beats an unbounded one that hangs the send path.
  return null;
}
