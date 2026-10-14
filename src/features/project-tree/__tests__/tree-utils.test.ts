/**
 * `src/features/project-tree/tree-utils.ts` 的纯单元测试 —— 打平节点列表 → 嵌套树
 * 的构建，以及它的搜索过滤。
 *
 * 父子矩阵给了接口*两个*合法父节点：集合，或模块。这几个用例钉住的正是后半个约定：
 *
 *   1. 模块自己的接口进入 `TreeBranch.endpoints`，带着支撑它们的 detail 行，且与集合
 *      并列而非嵌在集合里。
 *   2. 集合的接口仍落在它自己的 `CollectionBranch` 里。
 *   3. 搜索保留「自己名下接口命中」的模块，并滤掉不匹配的接口 —— 两个层级共用
 *      同一个判定式。
 *
 * 运行：
 *   npx tsx --test src/features/project-tree/__tests__/tree-utils.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildTree, filterTree } from '../tree-utils';
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
 * 一个项目，两种摆放同时在场：
 *
 *   m1
 *   ├── e_module   （直接挂在模块上，即无文件夹的情形）
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
    // detail 行跟着一起过来，行要从这条记录里画 method 药丸和 url。
    assert.equal(m1.endpointDetails.e_module.url, '/module-level');
    // 集合不受影响。
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
    // 递归完好：c1 的子集合保住了自己的接口。
    assert.deepEqual(
      c1.children[0].endpoints.map((n) => n.id),
      ['e_deep'],
    );
  });

  it('omits an endpoint whose detail row is missing rather than rendering undefined', () => {
    const nodes = [makeNode('m1', null, 'module'), makeNode('e_orphan', 'm1', 'endpoint')];
    const m1 = buildTree(nodes, {}, 'p1')[0];

    // 节点仍列出（树是结构性的），但没有 detail 可画，`EndpointList` 会跳过这一行。
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
