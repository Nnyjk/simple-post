/**
 * `src/lib/tree-index.ts`（共享的 `parentId → children` 索引）的纯单测。
 *
 * 索引只能有一份实现：组件里就地过滤扁平列表拿 children，等于重建一份。这里钉住
 * 调用方依赖的那一条契约：
 *
 *   1. 节点落进 `parentId` 指定的那个桶，`null` 自成一个顶层桶（module 的父级是
 *      项目，不是另一个节点）。
 *   2. 每个桶按 `sortOrder` 排序——是在*它自己的父级内*排，且与调用方碰巧给出的
 *      顺序无关。
 *   3. 可选的 `rootId` 收窄到单个项目，省略即整份工作区。
 *   4. 调用方拥有的东西一个都不重排、不写回——map 按引用持有调用方的节点对象，
 *      调用方自己的数组保持原序。
 *
 * 第 4 条正是 store 能在 `set()` 回调里建索引的依据；对应的另一个坑——放进 Zustand
 * selector 里建会无限重渲染——属于调用方规则，写在函数自身的注释里。
 *
 * 运行：
 *   npx tsx --test src/lib/__tests__/tree-index.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { indexByParent, moduleIdOfEndpoint } from '../tree-index';
import type { ResourceNode, ResourceNodeType } from '@/types/domain';

// ---------- 夹具 ----------

/** 最小化的领域形状节点；索引只读 `id` / `rootId` / `parentId` / `sortOrder`。 */
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

/** 某个桶的 id 列表，按桶内顺序——渲染器走的就是这个形状。 */
function idsOf(bucket: readonly ResourceNode[] | undefined): string[] {
  return (bucket ?? []).map((n) => n.id);
}

/** `parentId` 下每个桶的 id 列表，按桶内顺序。 */
function idsFor(index: Map<string | null, ResourceNode[]>, parentId: string | null): string[] {
  return idsOf(index.get(parentId));
}

// ============================================================
// 分桶
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

    // module 的父级是项目，所以顶层是 `parentId: null`，绝不会变成字符串 `'null'` 或某个项目 id。
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
// 排序
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
    // `sortOrder` 是*同级*内的位置，不是全项目内的；全局排序会错误地交错两个集合。
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
// 可选的项目过滤
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
    // `buildTree` 读某个项目的模块列表正是读这个；而 `moveNodes` 划定顶层落点范围时不能这么做。
    const p2 = indexByParent(crossProject, 'p2');
    assert.deepEqual(idsFor(p2, null), ['m9']);
    assert.deepEqual(idsFor(p2, 'm9'), ['c9']);
  });

  it('returns an empty index for a project with no nodes', () => {
    assert.equal(indexByParent(crossProject, 'p404').size, 0);
  });
});

// ============================================================
// 孤儿
// ============================================================

describe('tree-index: orphans', () => {
  it('still buckets a node whose parentId names a node absent from the input', () => {
    // 父节点可能只是被 `rootId` 过滤掉了，或在一次部分加载里缺席。丢掉子节点会让它在
    // store 里无法抵达，所以桶照建——遍历者要不要忽略它是另一个决定。
    // store 里无法抵达，所以桶照建——遍历者要不要忽略它是另一个决定。
    const index = indexByParent([makeNode('e1', 'missing-parent', 0)]);

    assert.deepEqual(idsFor(index, 'missing-parent'), ['e1']);
    assert.equal(index.size, 1);
  });
});

// ============================================================
// 纯度 —— 调用方的数组必须完好
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
    // 排序发生在刚创建的桶上，所以桶里是调用方的对象本身而不是它们的副本。
    assert.equal(index.get('c1')?.[0], nodes[1]);
  });
});

// ============================================================
// moduleIdOfEndpoint
// ============================================================

/**
 * 模块 → 集合 → endpoint 是深的那层；endpoint 直接挂在模块上是类型矩阵允许、树也
 * 会渲染的那种，所以遍历必须在两种形态上都终止。
 */
const NESTED_TREE: ResourceNode[] = [
  makeNode('mod_a', null, 0, { kind: 'module' }),
  makeNode('col_a', 'mod_a', 0, { kind: 'collection' }),
  makeNode('sub_a', 'col_a', 0, { kind: 'collection' }),
  makeNode('ep_deep', 'sub_a', 0),
  makeNode('ep_shallow', 'mod_a', 1),
  makeNode('mod_b', null, 1, { kind: 'module' }),
  makeNode('ep_b', 'mod_b', 0),
];

describe('tree-index: moduleIdOfEndpoint', () => {
  it('finds the module through arbitrarily nested collections', () => {
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'ep_deep'), 'mod_a');
  });

  it('finds the module for an endpoint sitting directly on it', () => {
    // endpoint 不需要自己的集合，所以只有一层那种是常见情形，不是边界情形。
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'ep_shallow'), 'mod_a');
  });

  it('does not confuse two modules', () => {
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'ep_b'), 'mod_b');
  });

  it('returns the module itself when handed one', () => {
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'mod_a'), 'mod_a');
  });

  it('returns null for an endpoint id that is not in the tree', () => {
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'nope'), null);
  });

  it('returns null for no endpoint at all', () => {
    // HTTP 测试器在 endpoint 还不存在时就渲染空状态；那时必须解析出项目级的 base URL，而不是抛错。
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, null), null);
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, undefined), null);
    assert.equal(moduleIdOfEndpoint([], 'ep_deep'), null);
  });

  it('terminates on a cyclic parent chain instead of hanging', () => {
    // schema 禁止这种情况，所以遍历可以当它不存在——但这里无界的循环会挂死发送路径，而有界的
    // 遍历答 null 只要三行。
    const cyclic = [
      makeNode('x', 'y', 0, { kind: 'collection' }),
      makeNode('y', 'x', 0, { kind: 'collection' }),
    ];
    assert.equal(moduleIdOfEndpoint(cyclic, 'x'), null);
  });

  it('resolves an unsaved draft from the node synthesised for it', () => {
    // 草稿 endpoint 只存在于 `draftEndpoints`——它不在 `nodes` 里，Save 之前也不会在——
    // 所以只按 id 查会得到 null，下游读起来就是「没有模块」，也就是没有 baseUrl 格子。
    // 它的各级父节点都是真实行，所以把草稿自身的节点作为遍历起点，就是补上了缺失的那一步。
    const draft = makeNode('draft_1', 'sub_a', 0);
    // NESTED_TREE 没有那行草稿——这正是重点——而它的父节点（sub_a）是再普通不过的一行。
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'draft_1'), null);
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'draft_1', draft), 'mod_a');
    // 直接挂在模块上，中间没有集合。
    assert.equal(
      moduleIdOfEndpoint(NESTED_TREE, 'draft_2', makeNode('draft_2', 'mod_b', 0)),
      'mod_b',
    );
  });

  it('ignores a `self` that is not the endpoint being asked about', () => {
    // 调用方在切换中途递来一个过期节点时，遍历不得从错误的 endpoint 出发、用它的模块作答。
    const draft = makeNode('draft_1', 'col_a', 0);
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'ep_b', draft), 'mod_b');
    assert.equal(moduleIdOfEndpoint(NESTED_TREE, 'draft_1', makeNode('other', 'mod_b', 0)), null);
  });
});
