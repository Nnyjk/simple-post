/**
 * 父子类型矩阵的一致性样本：TS 的 `src/lib/node-placement`（前端拖放与乐观更新）。
 *
 * 同一份样本也被 Rust 侧的
 * `crates/simple-post-core/src/placement_parity_test.rs` 读。两份实现各写一遍矩阵
 * （后端在每一次写入时执行它，前端在拖放判定与乐观更新时读它），只有一份共享样本才能
 * 钉住它们没分叉。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { ACCEPTED_CHILDREN, mayUnder } from '../node-placement';
import type { ResourceNodeType } from '../../types/domain';

interface ParityCell {
  child: string;
  parent: string;
  allowed: boolean;
}

interface ParityFixture {
  matrix: ParityCell[];
  projectTopLevel: string[];
}

const FIXTURE: ParityFixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/placement-parity.json', import.meta.url), 'utf8'),
);

const KINDS: ResourceNodeType[] = ['module', 'collection', 'endpoint'];

describe('前端读到的父子类型矩阵与 Rust 的 may_under 一致', () => {
  it('样本里至少有一个格子', () => {
    assert.ok(FIXTURE.matrix.length > 0, '样本里必须有格子，否则这条测试是空的');
  });

  for (const cell of FIXTURE.matrix) {
    it(`${cell.child} 挂在 ${cell.parent} 下：${cell.allowed ? '允许' : '不允许'}`, () => {
      assert.equal(
        mayUnder(cell.child as ResourceNodeType, cell.parent as ResourceNodeType),
        cell.allowed,
        `${cell.child} 能否挂在 ${cell.parent} 下`,
      );
    });
  }
});

describe('项目顶层只放得下模块', () => {
  for (const kind of KINDS) {
    it(kind, () => {
      const expected = FIXTURE.projectTopLevel.includes(kind);
      assert.equal(mayUnder(kind, 'project'), expected, `${kind} 能否落在项目顶层`);
    });
  }
});

describe('样本本身得是完整的', () => {
  it('每一格父子组合恰好出现一次', () => {
    for (const child of KINDS) {
      for (const parent of KINDS) {
        const count = FIXTURE.matrix.filter((c) => c.child === child && c.parent === parent).length;
        assert.equal(count, 1, `样本必须恰好覆盖一次「${child} 挂在 ${parent} 下」这一格`);
      }
    }
    assert.equal(FIXTURE.matrix.length, 9, '矩阵是 3 种子类型 × 3 种父类型 = 9 格');
  });

  it('表里没有样本之外的行', () => {
    assert.deepEqual(
      Object.keys(ACCEPTED_CHILDREN).sort(),
      ['collection', 'endpoint', 'module', 'project'],
      '表里多出或少了作用域 —— 样本只钉住节点之间的九格与项目顶层那一格',
    );
  });

  it('表里的每一行都恰好由样本决定', () => {
    for (const [scope, accepted] of Object.entries(ACCEPTED_CHILDREN)) {
      const expected =
        scope === 'project'
          ? KINDS.filter((k) => FIXTURE.projectTopLevel.includes(k))
          : KINDS.filter((k) =>
              FIXTURE.matrix.some((c) => c.child === k && c.parent === scope && c.allowed),
            );
      assert.deepEqual([...accepted].sort(), [...expected].sort(), `作用域 ${scope} 接受的子类型`);
    }
  });
});
