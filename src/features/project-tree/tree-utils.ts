import type { Collection, Endpoint, Module } from '@/types/domain';
import type { CollectionBranch, DragPayload, TreeBranch } from './tree-types';

/**
 * Custom mime type used for the in-app drag payload. We shove the dragged
 * kind + ids into the dataTransfer as JSON so a single drop can resolve
 * to `moveModule` / `moveCollections` / `moveEndpoints` without any
 * extra context. Using a vendor-prefixed type means other apps / the OS
 * can't accidentally pick the data up as text.
 */
export const DND_MIME = 'application/x-simple-post-tree';

export function encodeDrag(payload: DragPayload): string {
  return JSON.stringify(payload);
}

export function decodeDrag(raw: string): DragPayload | null {
  try {
    const parsed = JSON.parse(raw) as DragPayload;
    if (!parsed || typeof parsed !== 'object') return null;
    if (
      (parsed.kind === 'module' || parsed.kind === 'collection' || parsed.kind === 'endpoint') &&
      Array.isArray(parsed.ids) &&
      parsed.ids.every((id) => typeof id === 'string')
    ) {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

export function buildTree(
  modules: Module[],
  collections: Collection[],
  endpoints: Endpoint[],
  projectId: string,
): TreeBranch[] {
  // Bucket endpoints by collectionId once.
  const endpointsByCol = new Map<string, Endpoint[]>();
  for (const e of endpoints) {
    const bucket = endpointsByCol.get(e.collectionId);
    if (bucket) bucket.push(e);
    else endpointsByCol.set(e.collectionId, [e]);
  }
  for (const list of endpointsByCol.values()) {
    list.sort((a, b) => a.sortOrder - b.sortOrder);
  }
  // Bucket collections by parent (parentCollectionId can be null for
  // top-level). Only build the subtree for collections that belong to
  // this project.
  const projectModuleIds = new Set(
    modules.filter((m) => m.projectId === projectId).map((m) => m.id),
  );
  const colsByParent = new Map<string | null, Collection[]>();
  for (const c of collections) {
    if (!projectModuleIds.has(c.moduleId)) continue;
    const key = c.parentCollectionId;
    const bucket = colsByParent.get(key);
    if (bucket) bucket.push(c);
    else colsByParent.set(key, [c]);
  }
  for (const list of colsByParent.values()) {
    list.sort((a, b) => a.sortOrder - b.sortOrder);
  }

  const buildCollectionBranch = (c: Collection): CollectionBranch => ({
    collection: c,
    endpoints: endpointsByCol.get(c.id) ?? [],
    children: (colsByParent.get(c.id) ?? []).map(buildCollectionBranch),
  });

  return modules
    .filter((m) => m.projectId === projectId)
    .sort((a, b) => a.sortOrder - b.sortOrder)
    .map((m) => ({
      module: m,
      collections: (colsByParent.get(null) ?? [])
        .filter((c) => c.moduleId === m.id)
        .map(buildCollectionBranch),
    }));
}

export function countSubtreeEndpoints(branch: CollectionBranch): number {
  return (
    branch.endpoints.length + branch.children.reduce((acc, c) => acc + countSubtreeEndpoints(c), 0)
  );
}

export function countSubtreeCollections(branch: CollectionBranch): number {
  return branch.children.reduce((acc, c) => acc + 1 + countSubtreeCollections(c), 0);
}

export function countTreeEndpoints(branches: CollectionBranch[]): number {
  return branches.reduce((acc, b) => acc + countSubtreeEndpoints(b), 0);
}

export function filterTree(tree: TreeBranch[], q: string): TreeBranch[] {
  return tree
    .map((m) => {
      const collections = filterCollectionBranch(m.collections, q);
      return { ...m, collections };
    })
    .filter((m) => m.collections.length > 0);
}

function filterCollectionBranch(branches: CollectionBranch[], q: string): CollectionBranch[] {
  return branches
    .map((b) => {
      const matchedEndpoints = b.endpoints.filter(
        (e) =>
          e.name.toLowerCase().includes(q) ||
          e.url.toLowerCase().includes(q) ||
          e.tags.some((t) => t.toLowerCase().includes(q)),
      );
      const filteredChildren = filterCollectionBranch(b.children, q);
      // Keep a collection in the result if it has matched endpoints,
      // matched sub-collections, or its own name matches the query.
      const nameMatches = b.collection.name.toLowerCase().includes(q);
      if (matchedEndpoints.length === 0 && filteredChildren.length === 0 && !nameMatches) {
        return null;
      }
      return {
        ...b,
        endpoints: matchedEndpoints,
        children: filteredChildren,
      };
    })
    .filter((b): b is CollectionBranch => b !== null);
}
