/**
 * Pure unit tests for `src/lib/tree-index.ts` — the shared
 * `parentId → children` index.
 *
 * Why this file exists
 * --------------------
 * The index used to be written out twice (once in the CRUD store, once
 * inside `buildTree`), and the two copies could drift. These tests pin
 * the one contract both callers rely on:
 *
 *   1. Nodes land in the bucket their `parentId` names, with `null` as
 *      its own top-level bucket (a module's parent is the project, not
 *      another node).
 *   2. Each bucket is ordered by `sortOrder` — *within* its parent, and
 *      regardless of the order the caller happened to hand over.
 *   3. The optional `rootId` narrows to one project, and omitting it
 *      means the whole workspace.
 *   4. Nothing the caller owns is reordered or written to — the map
 *      holds the caller's node objects by reference, and the caller's
 *      own array keeps its order.
 *
 * Note (4) is what lets the store build an index inside a `set()`
 * callback; the counterpart hazard — building one inside a Zustand
 * selector, which would re-render forever — is a caller rule, documented
 * on the function itself.
 *
 * Run via:
 *   npx tsx --test src/lib/__tests__/tree-index.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { indexByParent } from '../tree-index';
import type { ResourceNode, ResourceNodeType } from '@/types/domain';

// ---------- fixtures ----------

/** Minimal domain-shaped node; the index only reads `id` / `rootId` / `parentId` / `sortOrder`. */
function makeNode(
  id: string,
  parentId: string | null,
  sortOrder: number,
  overrides: Partial<ResourceNode> = {},
): ResourceNode {
  return {
    id,
    rootId: 'p1',
    parentId,
    kind: 'endpoint' as ResourceNodeType,
    name: id,
    sortOrder,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

/** Ids of one bucket, in bucket order — the shape the renderer walks. */
function idsOf(bucket: readonly ResourceNode[] | undefined): string[] {
  return (bucket ?? []).map((n) => n.id);
}

/** Ids of every bucket for `parentId`, in bucket order. */
function idsFor(index: Map<string | null, ResourceNode[]>, parentId: string | null): string[] {
  return idsOf(index.get(parentId));
}

// ============================================================
// Bucketing
// ============================================================

describe('tree-index: bucketing by parentId', () => {
  it('puts each node in the bucket its parentId names', () => {
    const index = indexByParent([
      makeNode('m1', null, 0, { kind: 'module' }),
      makeNode('c1', 'm1', 0, { kind: 'collection' }),
      makeNode('e1', 'c1', 0),
    ]);

    assert.deepEqual(idsFor(index, null), ['m1']);
    assert.deepEqual(idsFor(index, 'm1'), ['c1']);
    assert.deepEqual(idsFor(index, 'c1'), ['e1']);
  });

  it('keeps `null` as its own top-level bucket, separate from any string parent', () => {
    const index = indexByParent([
      makeNode('c1', 'm1', 0),
      makeNode('m1', null, 0, { kind: 'module' }),
      makeNode('m2', null, 1, { kind: 'module' }),
    ]);

    // A module's parent is the project, so top level is `parentId: null`
    // and never the string `'null'` or a project id.
    assert.equal(index.get(null)?.length, 2);
    assert.deepEqual(idsFor(index, null), ['m1', 'm2']);
    assert.deepEqual(idsFor(index, 'm1'), ['c1']);
  });

  it('omits a bucket entirely when no node names that parent', () => {
    const index = indexByParent([makeNode('e1', 'c1', 0)]);
    assert.equal(index.has('c9'), false);
    assert.equal(index.size, 1);
  });

  it('returns an empty index for an empty node list', () => {
    const index = indexByParent([]);
    assert.equal(index.size, 0);
  });
});

// ============================================================
// Ordering
// ============================================================

describe('tree-index: sortOrder ordering', () => {
  it('sorts each bucket ascending by sortOrder regardless of input order', () => {
    const index = indexByParent([
      makeNode('e3', 'c1', 30),
      makeNode('e1', 'c1', 10),
      makeNode('e2', 'c1', 20),
    ]);

    assert.deepEqual(idsFor(index, 'c1'), ['e1', 'e2', 'e3']);
  });

  it('sorts top level too, not just nested buckets', () => {
    const index = indexByParent([
      makeNode('m2', null, 20, { kind: 'module' }),
      makeNode('m0', null, 5, { kind: 'module' }),
      makeNode('m1', null, 10, { kind: 'module' }),
    ]);

    assert.deepEqual(idsFor(index, null), ['m0', 'm1', 'm2']);
  });

  it('scopes the sort to the parent — the same sortOrder under two parents both survive', () => {
    // `sortOrder` is position among *siblings*, not across the project, so
    // a global sort would interleave the two collections incorrectly.
    const index = indexByParent([
      makeNode('a2', 'c1', 20),
      makeNode('b2', 'c2', 20),
      makeNode('a1', 'c1', 10),
      makeNode('b1', 'c2', 10),
    ]);

    assert.deepEqual(idsFor(index, 'c1'), ['a1', 'a2']);
    assert.deepEqual(idsFor(index, 'c2'), ['b1', 'b2']);
  });
});

// ============================================================
// The optional project filter
// ============================================================

describe('tree-index: optional rootId filter', () => {
  const crossProject = [
    makeNode('m1', null, 0, { kind: 'module' }),
    makeNode('c1', 'm1', 0, { kind: 'collection' }),
    makeNode('m9', null, 1, { kind: 'module', rootId: 'p2' }),
    makeNode('c9', 'm9', 0, { kind: 'collection', rootId: 'p2' }),
  ];

  it('drops other projects’ nodes when rootId is supplied', () => {
    const index = indexByParent(crossProject, 'p1');

    assert.deepEqual(
      [...index.values()]
        .flat()
        .map((n) => n.id)
        .sort(),
      ['c1', 'm1'],
    );
    assert.deepEqual(idsFor(index, null), ['m1']);
    assert.equal(index.has('m9'), false);
  });

  it('keeps every project when rootId is omitted', () => {
    const index = indexByParent(crossProject);

    assert.deepEqual(
      [...index.values()]
        .flat()
        .map((n) => n.id)
        .sort(),
      ['c1', 'c9', 'm1', 'm9'],
    );
    assert.deepEqual(idsFor(index, null), ['m1', 'm9']);
  });

  it('narrows the top-level bucket to the requested project’s modules only', () => {
    // This is exactly what `buildTree` reads for a project's module list,
    // and what `moveNodes` must not do when it scopes a top-level drop.
    const p2 = indexByParent(crossProject, 'p2');
    assert.deepEqual(idsFor(p2, null), ['m9']);
    assert.deepEqual(idsFor(p2, 'm9'), ['c9']);
  });

  it('returns an empty index for a project with no nodes', () => {
    assert.equal(indexByParent(crossProject, 'p404').size, 0);
  });
});

// ============================================================
// Orphans
// ============================================================

describe('tree-index: orphans', () => {
  it('still buckets a node whose parentId names a node absent from the input', () => {
    // The parent may simply be filtered out by `rootId`, or missing from a
    // partial load. Dropping the child would make it unreachable in the
    // store, so the bucket is built regardless — the walker's decision to
    // ignore it is a separate one.
    const index = indexByParent([makeNode('e1', 'missing-parent', 0)]);

    assert.deepEqual(idsFor(index, 'missing-parent'), ['e1']);
    assert.equal(index.size, 1);
  });
});

// ============================================================
// Purity — the caller's array must survive
// ============================================================

describe('tree-index: does not mutate its input', () => {
  it('leaves the caller’s array order untouched', () => {
    const nodes = [makeNode('e3', 'c1', 30), makeNode('e1', 'c1', 10), makeNode('e2', 'c1', 20)];
    const before = nodes.map((n) => n.id);

    indexByParent(nodes);

    assert.deepEqual(
      nodes.map((n) => n.id),
      before,
    );
  });

  it('leaves the caller’s node objects untouched and shares them by reference', () => {
    const nodes = [makeNode('e2', 'c1', 20), makeNode('e1', 'c1', 10)];
    const snapshot = nodes.map((n) => ({ ...n }));

    const index = indexByParent(nodes);

    assert.deepEqual(nodes, snapshot);
    // Sorting happens on the freshly created buckets, so the buckets hold
    // the caller's objects rather than copies of them.
    assert.equal(index.get('c1')?.[0], nodes[1]);
  });
});
