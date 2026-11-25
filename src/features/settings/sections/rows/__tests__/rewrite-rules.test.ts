/**
 * 有序路径改写规则的纯逻辑：拖拽落点、换序、可持久化的那一份、两份列表的等价判定。
 *
 * 这些判定写在 React 之外（见 `AGENTS.md` 的 Testing 一节：本仓没有组件测试），故这里是
 * 唯一能钉住它们的地方。`ModuleBaseUrlTable` 只把结果写进草稿。
 *
 * 运行：
 *   npx tsx --test src/features/settings/sections/rows/__tests__/rewrite-rules.test.ts
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import type { RewriteRule } from '@/types/domain';
import {
  areRewriteListsEqual,
  cleanPersistableRewrites,
  reorderRewriteRules,
  resolveRewriteDropZone,
} from '../rewrite-rules';

const rule = (from: string, to: string): RewriteRule => ({ from, to });

const ABC = [rule('/a', '/1'), rule('/b', '/2'), rule('/c', '/3')];

describe('resolveRewriteDropZone — 一行分上下两半', () => {
  const rect = { top: 100, height: 40 };

  it('上半区是 above，下半区是 below', () => {
    assert.equal(resolveRewriteDropZone(105, rect), 'above');
    assert.equal(resolveRewriteDropZone(119, rect), 'above');
    assert.equal(resolveRewriteDropZone(121, rect), 'below');
    assert.equal(resolveRewriteDropZone(139, rect), 'below');
  });
});

describe('reorderRewriteRules — 拖拽与上下移共用的换序', () => {
  it('把中间那条拖到第一条上面', () => {
    assert.deepEqual(reorderRewriteRules(ABC, 1, 0, 'above'), [ABC[1], ABC[0], ABC[2]]);
  });

  it('把第一条拖到最后一条下面', () => {
    assert.deepEqual(reorderRewriteRules(ABC, 0, 2, 'below'), [ABC[1], ABC[2], ABC[0]]);
  });

  it('把最后一条拖到第一条下面', () => {
    assert.deepEqual(reorderRewriteRules(ABC, 2, 0, 'below'), [ABC[0], ABC[2], ABC[1]]);
  });

  it('拖到相邻行的下半区等于下移一位', () => {
    assert.deepEqual(reorderRewriteRules(ABC, 0, 1, 'below'), [ABC[1], ABC[0], ABC[2]]);
  });

  it('拖到相邻行的上半区等于上移一位', () => {
    assert.deepEqual(reorderRewriteRules(ABC, 1, 0, 'above'), [ABC[1], ABC[0], ABC[2]]);
  });

  it('落点没有变化时返回原数组引用，调用方据此跳过空写', () => {
    assert.equal(reorderRewriteRules(ABC, 0, 1, 'above'), ABC);
    assert.equal(reorderRewriteRules(ABC, 0, 0, 'below'), ABC);
    assert.equal(reorderRewriteRules(ABC, 2, 2, 'above'), ABC);
  });

  it('下标越界时不动，且不抛', () => {
    assert.equal(reorderRewriteRules(ABC, -1, 1, 'above'), ABC);
    assert.equal(reorderRewriteRules(ABC, 0, 3, 'below'), ABC);
  });

  it('不改动传入的数组', () => {
    const copy = [...ABC];
    reorderRewriteRules(ABC, 0, 2, 'below');
    assert.deepEqual(ABC, copy);
  });

  it('只有一条规则时任何落点都不动', () => {
    const one = [rule('/a', '/1')];
    assert.equal(reorderRewriteRules(one, 0, 0, 'above'), one);
  });
});

describe('cleanPersistableRewrites — 全空白占位行不落库', () => {
  it('剔除起点与目标都空白的行，其余原样保留顺序', () => {
    const rules = [rule(' ', '  '), rule('/a', '/1'), rule('/a', ''), rule('', '')];
    assert.deepEqual(cleanPersistableRewrites(rules), [rule('/a', '/1'), rule('/a', '')]);
  });

  it('只填了一半的行留着：起点有内容、目标空表示去掉这段前缀', () => {
    assert.deepEqual(cleanPersistableRewrites([rule('/a', '')]), [rule('/a', '')]);
    assert.deepEqual(cleanPersistableRewrites([rule('', '/x')]), [rule('', '/x')]);
  });

  it('返回的是新数组，调用方的草稿不被改写', () => {
    const rules = [rule('/a', '/1')];
    const cleaned = cleanPersistableRewrites(rules);
    assert.notEqual(cleaned, rules);
  });
});

describe('areRewriteListsEqual — 逐条比较', () => {
  it('内容与顺序都相同时为真', () => {
    assert.equal(areRewriteListsEqual([rule('/a', '/1')], [rule('/a', '/1')]), true);
    assert.equal(areRewriteListsEqual([], []), true);
  });

  it('顺序不同即为假：顺序就是优先级', () => {
    assert.equal(
      areRewriteListsEqual(
        [rule('/a', '/1'), rule('/b', '/2')],
        [rule('/b', '/2'), rule('/a', '/1')],
      ),
      false,
    );
  });

  it('条数不同或有一处取值不同即为假', () => {
    assert.equal(areRewriteListsEqual([rule('/a', '/1')], []), false);
    assert.equal(areRewriteListsEqual([rule('/a', '/1')], [rule('/a', '/2')]), false);
  });
});
