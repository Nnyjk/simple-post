import type { EndpointDetail, ResourceNode } from '@/types/domain';
import { indexByParent } from '@/lib/tree-index';
import type { CollectionBranch, DragPayload, TreeBranch } from './tree-types';

/**
 * Custom mime type used for the in-app drag payload. We shove the dragged
 * kind + ids into the dataTransfer as JSON so a single drop can resolve
 * to `moveNodes` without any extra context. Using a vendor-prefixed type
 * means other apps / the OS can't accidentally pick the data up as text.
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

/**
 * Turn the flat node list into the nested shape the rows render.
 *
 * One pass. Every node names its own parent, so the whole tree is a
 * single `parentId → children` index — the old version needed three
 * because a collection's owner and its parent lived in different
 * columns, and a module's project was a fourth thing to look up.
 *
 * `projectId` narrows to one project's tree; nodes from other projects
 * are dropped rather than rendered under a module that is not theirs.
 */
export function buildTree(
  nodes: readonly ResourceNode[],
  endpointDetails: Record<string, EndpointDetail>,
  projectId: string,
): TreeBranch[] {
  // The bucketing itself is shared with the store — same index, narrowed
  // to this project rather than the whole workspace.
  const byParent = indexByParent(nodes, projectId);

  const childrenOf = (
    parentId: string,
  ): { endpoints: ResourceNode[]; collections: ResourceNode[] } => {
    const kids = byParent.get(parentId) ?? [];
    return {
      // One filter covers both container types: a module holds
      // collections *and* endpoints (the folderless case), and a
      // collection holds endpoints and further collections. Nothing else
      // can appear under either, so this needs no per-type query.
      endpoints: kids.filter((k) => k.kind === 'endpoint'),
      collections: kids.filter((k) => k.kind === 'collection'),
    };
  };

  /** The subset of the store's detail map covering `endpoints`. */
  const detailsFor = (endpoints: readonly ResourceNode[]): Record<string, EndpointDetail> => {
    const out: Record<string, EndpointDetail> = {};
    for (const e of endpoints) {
      const d = endpointDetails[e.id];
      if (d) out[e.id] = d;
    }
    return out;
  };

  const buildCollectionBranch = (c: ResourceNode): CollectionBranch => {
    const { endpoints, collections } = childrenOf(c.id);
    // Only this branch's own endpoints, not the whole subtree — the
    // rows render exactly these.
    return {
      collection: c,
      endpoints,
      endpointDetails: detailsFor(endpoints),
      children: collections.map(buildCollectionBranch),
    };
  };

  // A module's parent is the project, so top-level modules are the nodes
  // with no `parentId` in this project.
  return (byParent.get(null) ?? [])
    .filter((n) => n.kind === 'module')
    .map((m) => {
      const { endpoints, collections } = childrenOf(m.id);
      return {
        module: m,
        endpoints,
        endpointDetails: detailsFor(endpoints),
        collections: collections.map(buildCollectionBranch),
      };
    });
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

/**
 * Every endpoint under a module: the ones sitting on it directly plus the
 * ones inside its collections. A module holds both, so counting only the
 * collections' would under-report every module that has a folderless
 * endpoint.
 */
export function countModuleEndpoints(branch: TreeBranch): number {
  return branch.endpoints.length + countTreeEndpoints(branch.collections);
}

/**
 * Does this endpoint match the search? Name, url or any tag — url and
 * tags live in the detail row, so a name-only match over the node would
 * silently stop finding endpoints by path.
 *
 * Shared by the module-level and collection-level filters so the two
 * levels can never drift into matching different things.
 */
function endpointMatches(
  e: ResourceNode,
  q: string,
  details: Record<string, EndpointDetail>,
): boolean {
  if (e.name.toLowerCase().includes(q)) return true;
  const d = details[e.id];
  if (!d) return false;
  return d.url.toLowerCase().includes(q) || d.tags.some((t) => t.toLowerCase().includes(q));
}

/** Keep only the matched endpoints, with the detail rows that back them. */
function filterEndpoints(
  endpoints: ResourceNode[],
  q: string,
  details: Record<string, EndpointDetail>,
): { endpoints: ResourceNode[]; endpointDetails: Record<string, EndpointDetail> } {
  const matched = endpoints.filter((e) => endpointMatches(e, q, details));
  const filteredDetails: Record<string, EndpointDetail> = {};
  for (const e of matched) {
    const d = details[e.id];
    if (d) filteredDetails[e.id] = d;
  }
  return { endpoints: matched, endpointDetails: filteredDetails };
}

export function filterTree(
  tree: TreeBranch[],
  q: string,
  details: Record<string, EndpointDetail>,
): TreeBranch[] {
  return (
    tree
      .map((m) => {
        const collections = filterCollectionBranch(m.collections, q, details);
        // A module can own endpoints directly, so filtering its
        // collections alone would make them unfindable.
        const { endpoints, endpointDetails } = filterEndpoints(m.endpoints, q, details);
        return { ...m, collections, endpoints, endpointDetails };
      })
      // A module survives on a matched collection *or* a matched endpoint
      // of its own.
      .filter((m) => m.collections.length > 0 || m.endpoints.length > 0)
  );
}

function filterCollectionBranch(
  branches: CollectionBranch[],
  q: string,
  details: Record<string, EndpointDetail>,
): CollectionBranch[] {
  return branches
    .map((b) => {
      const { endpoints, endpointDetails } = filterEndpoints(b.endpoints, q, details);
      const filteredChildren = filterCollectionBranch(b.children, q, details);
      // Keep a collection in the result if it has matched endpoints,
      // matched sub-collections, or its own name matches the query.
      const nameMatches = b.collection.name.toLowerCase().includes(q);
      if (endpoints.length === 0 && filteredChildren.length === 0 && !nameMatches) {
        return null;
      }
      return {
        ...b,
        endpoints,
        endpointDetails,
        children: filteredChildren,
      };
    })
    .filter((b): b is CollectionBranch => b !== null);
}
