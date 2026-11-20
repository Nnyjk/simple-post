import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  BULK_EXPAND_WINDOW_MS,
  REVEAL_AMOUNT,
  REVEAL_EACH,
  REVEAL_MAX_ROWS,
  isBulkExpandFresh,
  markBulkExpand,
  revealPlan,
} from '../tree-motion';

describe('revealPlan', () => {
  it('200 行的集合只排前几行，总错峰时长封顶', () => {
    const plan = revealPlan(200);
    assert.equal(plan.rows, REVEAL_MAX_ROWS);
    assert.ok(plan.amount <= REVEAL_AMOUNT);
  });

  it('行数增长时总时长不缩、不越过上限', () => {
    let previous = 0;
    for (const count of [0, 1, 2, 3, 5, 8, 50, 200]) {
      const { amount } = revealPlan(count);
      assert.ok(amount >= previous, `${count} 行的总时长比更少的行还短`);
      assert.ok(amount <= REVEAL_AMOUNT, `${count} 行的总时长越过了上限`);
      previous = amount;
    }
  });

  it('少量行按 each 铺开，总时长仍在上限内', () => {
    const plan = revealPlan(3);
    assert.equal(plan.rows, 3);
    assert.equal(plan.each, REVEAL_EACH);
    assert.ok(plan.amount < REVEAL_AMOUNT);
  });

  it('0 行与负数不产生错峰', () => {
    assert.equal(revealPlan(0).amount, 0);
    assert.equal(revealPlan(-3).rows, 0);
    assert.equal(revealPlan(-3).amount, 0);
  });
});

describe('批量展开窗口', () => {
  it('标记之后窗口内为真，越过窗口即失效', () => {
    markBulkExpand(1000);
    assert.equal(isBulkExpandFresh(1000), true);
    assert.equal(isBulkExpandFresh(1000 + BULK_EXPAND_WINDOW_MS), true);
    assert.equal(isBulkExpandFresh(1001 + BULK_EXPAND_WINDOW_MS), false);
  });
});
