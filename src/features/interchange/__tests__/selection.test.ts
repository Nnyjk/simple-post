import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import type { TransferEndpoint, TransferNode } from '@/lib/interchange-types';
import {
  allKeys,
  countSelected,
  groupState,
  invert,
  isIndeterminate,
  keysUnder,
  selectAll,
  selectNone,
  toggleEndpoint,
  toggleGroup,
} from '../selection';

function ep(key: string): TransferEndpoint {
  return {
    key,
    name: key,
    method: 'GET',
    path: `/${key}`,
    bodyMode: 'none',
    auth: 'none',
    duplicate: false,
    diagnosticIndexes: [],
  };
}

function node(key: string, endpoints: string[], children: TransferNode[] = []): TransferNode {
  return {
    key,
    name: key,
    endpoints: endpoints.map(ep),
    children,
    endpointTotal: endpoints.length + children.reduce((n, c) => n + c.endpointTotal, 0),
  };
}

// 一个两层树：m 下直接 2 个接口 + 两个子分组，各 1 个。
const TREE: TransferNode[] = [node('m', ['a', 'b'], [node('c0', ['c']), node('c1', ['d'])])];

describe('keysUnder / allKeys', () => {
  it('走遍每一层且顺序稳定', () => {
    assert.deepEqual(keysUnder(TREE[0]), ['a', 'b', 'c', 'd']);
    assert.deepEqual(allKeys(TREE), ['a', 'b', 'c', 'd']);
    // 顺序稳定是反选与回填的前提：两次遍历必须给出同一个序列。
    assert.deepEqual(allKeys(TREE), allKeys(TREE));
  });

  it('空分组不贡献任何 key', () => {
    assert.deepEqual(allKeys([node('empty', [])]), []);
  });
});

describe('groupState', () => {
  it('空分组一律 none', () => {
    assert.equal(groupState(node('e', []), new Set()), 'none');
    // 即便「被选中」也不该是 all——它没有可勾的东西。
    assert.equal(groupState(node('e', []), new Set()), 'none');
  });

  it('全选 / 部分 / 全不选三态', () => {
    assert.equal(groupState(TREE[0], new Set()), 'none');
    assert.equal(groupState(TREE[0], new Set(['a'])), 'some');
    assert.equal(groupState(TREE[0], new Set(['a', 'b'])), 'some');
    assert.equal(groupState(TREE[0], new Set(['a', 'b', 'c', 'd'])), 'all');
  });

  it('三态只看子树，不受外部选中影响', () => {
    const outside = new Set(['zzz', 'a', 'b', 'c', 'd']);
    assert.equal(groupState(TREE[0], outside), 'all');
  });
});

describe('全选 / 全不选 / 反选', () => {
  it('全选覆盖每一个层级', () => {
    assert.deepEqual([...selectAll(new Set(), TREE)].sort(), ['a', 'b', 'c', 'd']);
    assert.equal(countSelected(selectAll(new Set(), TREE), TREE), 4);
  });

  it('全不选清空一切', () => {
    assert.equal(countSelected(selectNone(new Set(['a', 'b', 'c', 'd']), TREE), TREE), 0);
  });

  it('反选双向翻转：已选的取消、没选的补上', () => {
    // 从「全没选」反选 → 全选。
    assert.deepEqual([...invert(new Set(), TREE)].sort(), ['a', 'b', 'c', 'd']);
    // 从「选了 a 与 c」反选 → 只剩 b 与 d。这两条一起才证明它不是「全选或全不选」。
    const partial = new Set(['a', 'c']);
    assert.deepEqual([...invert(partial, TREE)].sort(), ['b', 'd']);
  });

  it('连点两次反选回到原样', () => {
    const once = invert(new Set(['a']), TREE);
    assert.deepEqual([...invert(once, TREE)].sort(), ['a']);
  });

  it('反选不改动传入的集合', () => {
    const before = new Set(['a']);
    invert(before, TREE);
    assert.deepEqual([...before], ['a']);
  });
});

describe('toggleGroup', () => {
  it('全不选 → 全选；部分 → 补满；全选 → 全不选', () => {
    assert.deepEqual([...toggleGroup(TREE[0], new Set())].sort(), ['a', 'b', 'c', 'd']);
    assert.deepEqual([...toggleGroup(TREE[0], new Set(['a']))].sort(), ['a', 'b', 'c', 'd']);
    assert.equal(toggleGroup(TREE[0], new Set(['a', 'b', 'c', 'd'])).size, 0);
  });

  it('只影响该分组，不碰别的分组', () => {
    const other = new Set(['zzz']);
    const next = toggleGroup(node('c0', ['c']), other);
    assert.equal(next.has('zzz'), true);
    assert.equal(next.has('c'), true);
  });

  it('空分组点了也不会凭空造出选中项', () => {
    assert.equal(toggleGroup(node('e', []), new Set()).size, 0);
  });
});

describe('toggleEndpoint 与 indeterminate', () => {
  it('切换单个 key', () => {
    const on = toggleEndpoint('a', new Set());
    assert.equal(on.has('a'), true);
    assert.equal(toggleEndpoint('a', on).has('a'), false);
  });

  it('分组行只在「部分选中」时是半选', () => {
    assert.equal(isIndeterminate(TREE[0], new Set(['a'])), true);
    assert.equal(isIndeterminate(TREE[0], new Set()), false);
    assert.equal(isIndeterminate(TREE[0], new Set(['a', 'b', 'c', 'd'])), false);
  });
});
