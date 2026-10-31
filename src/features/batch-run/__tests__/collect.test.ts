import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { collectSubtreeEndpoints, countSubtree } from '../model/collect';
import type { ResourceNode } from '@/types/domain';

/**
 * 共享样本：集合子树的端点清单与顺序。
 *
 * Rust 侧的 `db::nodes::ordered_subtree_endpoints` 读同一份文件。两侧各写一遍遍历，
 * 故只有一份共享样本才能钉住它们没分叉。
 */
interface FixtureNode {
  id: string;
  rootId: string;
  parentId: string | null;
  kind: ResourceNode['kind'];
  name: string;
  sortOrder: number;
}

interface FixtureCase {
  name: string;
  root: string;
  nodes: FixtureNode[];
  expected: string[];
}

interface Fixture {
  cases: FixtureCase[];
}

const FIXTURE: Fixture = JSON.parse(
  readFileSync(
    new URL('../../../../fixtures/collection-order-parity.json', import.meta.url),
    'utf8',
  ),
);

function toNode(n: FixtureNode): ResourceNode {
  return {
    id: n.id,
    rootId: n.rootId,
    parentId: n.parentId,
    kind: n.kind,
    name: n.name,
    sortOrder: n.sortOrder,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe('collectSubtreeEndpoints 与共享样本一致', () => {
  it('样本本身不是空的 —— 空的样本钉不住任何东西', () => {
    assert.ok(FIXTURE.cases.length > 0, '共享样本里至少要有一条用例');
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      const nodes = c.nodes.map(toNode);
      const got = collectSubtreeEndpoints(nodes, c.root).map((n) => n.id);
      assert.deepEqual(got, c.expected);
    });
  }
});

describe('collectSubtreeEndpoints', () => {
  const nodes: ResourceNode[] = [
    {
      id: 'm1',
      rootId: 'p1',
      parentId: null,
      kind: 'module',
      name: '核心',
      sortOrder: 0,
      createdAt: 1,
      updatedAt: 1,
    },
    {
      id: 'c1',
      rootId: 'p1',
      parentId: 'm1',
      kind: 'collection',
      name: '订单',
      sortOrder: 0,
      createdAt: 1,
      updatedAt: 1,
    },
    {
      id: 'e1',
      rootId: 'p1',
      parentId: 'c1',
      kind: 'endpoint',
      name: '列表',
      sortOrder: 0,
      createdAt: 1,
      updatedAt: 1,
    },
  ];

  it('根不存在时返回空列表，不抛', () => {
    assert.deepEqual(collectSubtreeEndpoints(nodes, 'nope'), []);
  });

  it('父子关系成环时不会绕圈', () => {
    const cyclic: ResourceNode[] = [
      {
        id: 'c1',
        rootId: 'p1',
        parentId: 'c2',
        kind: 'collection',
        name: '甲',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: 'c2',
        rootId: 'p1',
        parentId: 'c1',
        kind: 'collection',
        name: '乙',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
    ];
    assert.deepEqual(collectSubtreeEndpoints(cyclic, 'c1'), []);
  });
});

describe('countSubtree', () => {
  it('数出接口数与直接子集合数', () => {
    const nodes: ResourceNode[] = [
      {
        id: 'm1',
        rootId: 'p1',
        parentId: null,
        kind: 'module',
        name: '核心',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: 'c1',
        rootId: 'p1',
        parentId: 'm1',
        kind: 'collection',
        name: '订单',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: 'c2',
        rootId: 'p1',
        parentId: 'c1',
        kind: 'collection',
        name: '退款',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: 'e1',
        rootId: 'p1',
        parentId: 'c1',
        kind: 'endpoint',
        name: '列表',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: 'e2',
        rootId: 'p1',
        parentId: 'c2',
        kind: 'endpoint',
        name: '发起退款',
        sortOrder: 0,
        createdAt: 1,
        updatedAt: 1,
      },
    ];
    const count = countSubtree(nodes, 'c1');
    assert.equal(count.endpoints, 2, '接口数要算上嵌套子集合里的');
    assert.equal(count.collections, 1, '子集合数只数直接的一层');
  });
});
