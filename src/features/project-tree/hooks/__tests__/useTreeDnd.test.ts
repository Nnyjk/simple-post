/**
 * `src/features/project-tree/hooks/useTreeDnd.ts` 里 `zoneFromRowPosition` 的单元测试
 * —— 树判定放置区的纯核心。
 *
 * 分区计算从行自身的 bounding rect 出发，rect 不可用时只会退化成常量答案且不报错。
 * 故规则本身在此钉住：给定真实 rect，三个区必须都可达，且分界要匹配该行的嵌套能力。
 *
 * 运行：
 *   npx tsx --test src/features/project-tree/hooks/__tests__/useTreeDnd.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { zoneFromRowPosition } from '../useTreeDnd';

// ---------- fixtures ----------

/** 高 100px、位于视口下方 200px 起的一行。 */
const ROW = { top: 200, height: 100 };

/** `ROW` 内给定纵向比例时的光标 Y。 */
function yAt(ratio: number): number {
  return ROW.top + ratio * ROW.height;
}

// ============================================================
// container rows — 25/50/25
// ============================================================

describe('useTreeDnd: zoneFromRowPosition on a container row', () => {
  const supportsInto = true;

  it('answers above at the top edge', () => {
    assert.equal(zoneFromRowPosition(ROW, yAt(0), supportsInto), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.1), supportsInto), 'above');
    // 0.28 是上四分位不含的上界。
    assert.equal(zoneFromRowPosition(ROW, yAt(0.27), supportsInto), 'above');
  });

  it('answers into across the middle half — the "nest under it" band', () => {
    assert.equal(zoneFromRowPosition(ROW, yAt(0.28), supportsInto), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.5), supportsInto), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.72), supportsInto), 'into');
  });

  it('answers below at the bottom edge', () => {
    // 0.72 是下四分位不含的下界。
    assert.equal(zoneFromRowPosition(ROW, yAt(0.73), supportsInto), 'below');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.99), supportsInto), 'below');
  });

  it('reaches every zone, so neither nesting nor downward reorder can go missing', () => {
    // 本文件要防的回归：答案恒定意味着三个区里必有一个是死代码，光标怎么动都到不了。
    const zones = new Set(
      [0.05, 0.5, 0.95].map((r) => zoneFromRowPosition(ROW, yAt(r), supportsInto)),
    );
    assert.deepEqual([...zones].sort(), ['above', 'below', 'into']);
  });
});

// ============================================================
// leaf rows — 50/50
// ============================================================

describe('useTreeDnd: zoneFromRowPosition on a row that cannot nest', () => {
  const supportsInto = false;

  it('splits evenly and never answers into', () => {
    assert.equal(zoneFromRowPosition(ROW, yAt(0), supportsInto), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.49), supportsInto), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.5), supportsInto), 'below');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.99), supportsInto), 'below');
  });

  it('reaches both above and below, so reordering is not stuck above the target', () => {
    const zones = new Set([0.1, 0.9].map((r) => zoneFromRowPosition(ROW, yAt(r), supportsInto)));
    assert.deepEqual([...zones].sort(), ['above', 'below']);
  });

  it('does not widen the split even in the middle of the row', () => {
    // 容器的阈值是 0.28 / 0.72；叶子行不能继承它们，否则指针停在行中间就会去把节点
    // 嵌进一个接口里。
    assert.notEqual(zoneFromRowPosition(ROW, yAt(0.5), supportsInto), 'into');
  });
});

// ============================================================
// degenerate rects
// ============================================================

describe('useTreeDnd: zoneFromRowPosition without a usable rect', () => {
  it('falls back to above for a missing rect', () => {
    assert.equal(zoneFromRowPosition(null, yAt(0.5), true), 'above');
    assert.equal(zoneFromRowPosition(undefined, yAt(0.5), true), 'above');
  });

  it('falls back to above for a zero-height rect rather than dividing by zero', () => {
    // height 为 0 会让比例变成 ±Infinity / NaN，而 NaN 满足不了任何比较，故须显式回落。
    assert.equal(zoneFromRowPosition({ top: 200, height: 0 }, 200, true), 'above');
    assert.equal(zoneFromRowPosition({ top: 200, height: 0 }, 200, false), 'above');
  });

  it('is viewport-relative, not page-relative', () => {
    // clientY 是视口坐标，rect 也必须按视口读：树滚动后仍在同样的光标高度处分界。
    const scrolled = { top: 40, height: 100 };
    assert.equal(zoneFromRowPosition(scrolled, 90, true), 'into');
    assert.equal(zoneFromRowPosition(scrolled, 130, true), 'below');
  });
});
