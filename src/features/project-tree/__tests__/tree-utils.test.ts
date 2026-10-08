/**
 * Pure unit tests for `src/features/project-tree/tree-utils.ts` — the
 * flat-node-list → nested-tree builder and its search filter.
 *
 * Why this file exists
 * --------------------
 * The parent/child matrix gives an endpoint *two* legal parents: a
 * collection, or a module. `buildTree` used to model only the first —
 * it mapped a module to its collections and dropped the rest — so a
 * folderless endpoint would have been written to the database and then
 * rendered nowhere. These tests pin the second half of the contract:
 *
 *   1. A module's own endpoints reach `TreeBranch.endpoints`, with the
 *      detail rows that back them, beside (not inside) its collections.
 *   2. A collection's endpoints still land in its `CollectionBranch`.
 *   3. Search keeps a module whose *own* endpoint matched, and filters
 *      its non-matching endpoints away — the same predicate both levels
 *      share.
 *   4. `countModuleEndpoints` counts both levels, so a module badge does
 *      not under-report.
 *
 * Run via:
 *   npx tsx --test src/features/project-tree/__tests__/tree-utils.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildTree, countModuleEndpoints, filterTree } from '../tree-utils';
import type { EndpointDetail, ResourceNode, ResourceNodeType } from '@/types/domain';

// ---------- fixtures ----------

function makeNode(
  id: string,
  parentId: string | null,
  kind: ResourceNodeType,
  sortOrder = 0,
): ResourceNode {
  return {
    id,
    rootId: 'p1',
    parentId,
    kind,
    name: id,
    sortOrder,
    createdAt: 0,
    updatedAt: 0,
  };
}

function makeDetail(id: string, url: string, tags: string[] = []): EndpointDetail {
  return {
    nodeId: id,
    method: 'GET',
    url,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags,
  };
}

/**
 * One project holding both placements at once:
 *
 *   m1
 *   ├── e_module   (directly on the module — the folderless case)
 *   └── c1
 *       ├── e_nested
 *       └── c2
 *           └── e_deep
 */
function mixedTree(): { nodes: ResourceNode[]; details: Record<string, EndpointDetail> } {
  return {
    nodes: [
      makeNode('m1', null, 'module'),
      makeNode('e_module', 'm1', 'endpoint'),
      makeNode('c1', 'm1', 'collection'),
      makeNode('e_nested', 'c1', 'endpoint'),
      makeNode('c2', 'c1', 'collection'),
      makeNode('e_deep', 'c2', 'endpoint'),
    ],
    details: {
      e_module: makeDetail('e_module', '/module-level'),
      e_nested: makeDetail('e_nested', '/nested'),
      e_deep: makeDetail('e_deep', '/deep'),
    },
  };
}

// ============================================================
// buildTree
// ============================================================

describe('tree-utils: buildTree keeps endpoints on their module', () => {
  it('puts a module-level endpoint on the branch, not inside a collection', () => {
    const { nodes, details } = mixedTree();
    const tree = buildTree(nodes, details, 'p1');

    assert.equal(tree.length, 1);
    const m1 = tree[0];
    assert.equal(m1.module.id, 'm1');
    assert.deepEqual(
      m1.endpoints.map((n) => n.id),
      ['e_module'],
    );
    // The detail row travels with it — the row renders a method pill and
    // a url from that record.
    assert.equal(m1.endpointDetails.e_module.url, '/module-level');
    // Collections are unaffected.
    assert.deepEqual(
      m1.collections.map((c) => c.collection.id),
      ['c1'],
    );
  });

  it('still nests a collection endpoint under its collection', () => {
    const { nodes, details } = mixedTree();
    const m1 = buildTree(nodes, details, 'p1')[0];

    const c1 = m1.collections[0];
    assert.deepEqual(
      c1.endpoints.map((n) => n.id),
      ['e_nested'],
    );
    assert.equal(c1.endpointDetails.e_nested.url, '/nested');
    // Recursion is intact: c1's sub-collection keeps its own endpoint.
    assert.deepEqual(
      c1.children[0].endpoints.map((n) => n.id),
      ['e_deep'],
    );
  });

  it('omits an endpoint whose detail row is missing rather than rendering undefined', () => {
    const nodes = [makeNode('m1', null, 'module'), makeNode('e_orphan', 'm1', 'endpoint')];
    const m1 = buildTree(nodes, {}, 'p1')[0];

    // The node is listed (the tree is structural), but there is no detail
    // to draw, and `EndpointList` skips the row.
    assert.deepEqual(
      m1.endpoints.map((n) => n.id),
      ['e_orphan'],
    );
    assert.deepEqual(m1.endpointDetails, {});
  });

  it('leaves a module with no endpoints of its own empty', () => {
    const nodes = [makeNode('m1', null, 'module'), makeNode('c1', 'm1', 'collection')];
    const m1 = buildTree(nodes, {}, 'p1')[0];

    assert.deepEqual(m1.endpoints, []);
  });
});

// ============================================================
// filterTree
// ============================================================

describe('tree-utils: filterTree reaches module-level endpoints', () => {
  it('keeps a module whose own endpoint matched', () => {
    const { nodes, details } = mixedTree();
    const tree = buildTree(nodes, details, 'p1');

    const found = filterTree(tree, 'module-level', details);
    assert.equal(found.length, 1, 'the module must survive on its own endpoint');
    assert.deepEqual(
      found[0].endpoints.map((n) => n.id),
      ['e_module'],
    );
    assert.equal(found[0].endpointDetails.e_module.url, '/module-level');
  });

  it('matches a module endpoint by name as well as by url', () => {
    const { nodes, details } = mixedTree();
    const tree = buildTree(nodes, details, 'p1');

    const found = filterTree(tree, 'e_module', details);
    assert.deepEqual(
      found[0].endpoints.map((n) => n.id),
      ['e_module'],
    );
  });

  it('drops non-matching module endpoints while keeping the matched collection branch', () => {
    const { nodes, details } = mixedTree();
    const tree = buildTree(nodes, details, 'p1');

    const found = filterTree(tree, 'deep', details);
    assert.equal(found[0].endpoints.length, 0, 'e_module does not match "deep"');
    assert.deepEqual(
      found[0].collections.map((c) => c.collection.id),
      ['c1'],
    );
    assert.deepEqual(
      found[0].collections[0].children[0].endpoints.map((n) => n.id),
      ['e_deep'],
    );
  });

  it('drops a module with no match at all', () => {
    const { nodes, details } = mixedTree();
    const tree = buildTree(nodes, details, 'p1');

    assert.deepEqual(filterTree(tree, 'nothing-matches-this', details), []);
  });
});

// ============================================================
// counts
// ============================================================

describe('tree-utils: countModuleEndpoints', () => {
  it('counts the module-level endpoints and the collections’ ones together', () => {
    const { nodes, details } = mixedTree();
    const m1 = buildTree(nodes, details, 'p1')[0];

    // e_module + e_nested + e_deep.
    assert.equal(countModuleEndpoints(m1), 3);
  });

  it('is zero for an empty module', () => {
    const nodes = [makeNode('m1', null, 'module')];
    assert.equal(countModuleEndpoints(buildTree(nodes, {}, 'p1')[0]), 0);
  });
});
