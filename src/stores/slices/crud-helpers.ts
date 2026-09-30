/**
 * Pure helpers shared by the CRUD sub-factories in this directory.
 *
 * They live here — store-side — rather than in `@/lib/tree-index` on
 * purpose: every one of them has exactly one or two callers, all inside
 * the CRUD slice, and promoting a single-use helper into a shared module
 * inflates that module's surface for every other importer. `indexByParent`
 * is the opposite case: it genuinely is shared, so the sub-factories keep
 * importing it from `@/lib/tree-index`.
 *
 * Everything below except `invokeWithRollback` is a pure
 * compute-over-arguments with no store access, which is what makes them
 * safe to call from inside a `set()` callback.
 */
import type { EndpointDetail, ResourceNode } from '@/types/domain';
import type { OpenTab } from '../store-types';

/**
 * The tail of every optimistic write: invoke the daemon, and on failure
 * log and roll the local state back.
 *
 * All ~15 write actions had this shape open-coded —
 *
 *     try { await invokeCmd(COMMANDS.x, args); }
 *     catch (e) { console.error('[store] x failed', e); set(rollback); }
 *
 * — which meant the log prefix and the "did you remember to roll back?"
 * question had to be re-answered at every site. This owns the try/catch
 * and the message format; the caller supplies only what "roll back"
 * means for that particular write.
 *
 * `rollback` is a callback rather than a value because the rollback
 * shapes genuinely differ: some are an updater (`set((s) => …)`) that has
 * to run against the *current* state, and some are a literal snapshot
 * object captured before the optimistic write. A helper that accepted a
 * single form would have forced the other kind to be rewritten.
 *
 * Note this deliberately does NOT swallow the error into a return value —
 * callers that need a value on failure (the create actions) keep their
 * own try/catch and return `null`.
 */
export async function invokeWithRollback(
  label: string,
  invoke: () => Promise<unknown>,
  rollback: () => void,
): Promise<void> {
  try {
    await invoke();
  } catch (e) {
    console.error(`[store] ${label} failed`, e);
    rollback();
  }
}

/** Re-number sortOrder for a slice after a reorder. */
export function renumber<T extends { sortOrder: number }>(items: readonly T[]): T[] {
  return items.map((it, idx) => ({ ...it, sortOrder: idx }));
}

/**
 * A node and every descendant beneath it.
 *
 * BFS over `parentId` with a visited set. The set is not defensive
 * padding: a cycle in the data would otherwise spin here forever, and
 * the UI is the only thing standing between the store and a
 * hand-edited `store.db`.
 */
export function collectSubtree(
  rootId: string,
  childrenIndex: Map<string | null, ResourceNode[]>,
): Set<string> {
  const out = new Set<string>();
  const queue: string[] = [rootId];
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (out.has(id)) continue;
    out.add(id);
    for (const kid of childrenIndex.get(id) ?? []) queue.push(kid.id);
  }
  return out;
}

/**
 * Drop tabs whose underlying entity was removed. Returns the new tab list,
 * the new active tab id (picking the previous neighbor if the active one
 * was removed), and a flag indicating whether anything changed.
 */
export function filterTabsAfterRemoval(
  tabs: readonly OpenTab[],
  activeId: string | null,
  closeSet: ReadonlySet<string>,
): { tabs: OpenTab[]; activeId: string | null; changed: boolean } {
  const nextTabs: OpenTab[] = [];
  for (const t of tabs) {
    if (closeSet.has(`${t.kind}:${t.id}`)) continue;
    nextTabs.push(t);
  }
  if (nextTabs.length === tabs.length) {
    return { tabs: tabs.slice(), activeId, changed: false };
  }

  let nextActive = activeId;
  if (activeId !== null && !nextTabs.some((t) => t.id === activeId)) {
    const oldIdx = tabs.findIndex((t) => t.id === activeId);
    // Prefer the previous tab (left neighbor), fall back to next, then null.
    const fallback = tabs[oldIdx - 1] ?? tabs[oldIdx + 1] ?? nextTabs[nextTabs.length - 1] ?? null;
    nextActive = fallback ? fallback.id : null;
  }
  return { tabs: nextTabs, activeId: nextActive, changed: true };
}

/**
 * The tab keys to close when a subtree is deleted.
 *
 * A tab is keyed by the id of whatever it shows, and a `module` /
 * `collection` / `endpoint` tab's id is its node id — so one pass over
 * the removed ids answers all three at once. Projects are separate:
 * they are not nodes, so they are closed by the project action.
 */
export function tabKeysForSubtree(
  nodes: readonly ResourceNode[],
  removed: ReadonlySet<string>,
  scope: 'project' | 'node',
  targetId: string,
): Set<string> {
  const close = new Set<string>();
  for (const n of nodes) {
    if (removed.has(n.id)) close.add(`${n.kind}:${n.id}`);
  }
  if (scope === 'project') close.add(`project:${targetId}`);
  return close;
}

/** The `EndpointDetail` an optimistic endpoint create starts from. */
export function blankDetail(
  nodeId: string,
  partial?: Partial<Omit<EndpointDetail, 'nodeId'>>,
): EndpointDetail {
  return {
    nodeId,
    method: 'GET',
    url: '',
    environmentId: null,
    baseUrlDefinitionId: null,
    baseUrlOverride: null,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags: [],
    ...partial,
  };
}
