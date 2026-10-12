/**
 * Unit tests for `zoneFromRowPosition` in
 * `src/features/project-tree/hooks/useTreeDnd.ts` — the pure core of
 * the tree's drop-zone resolution.
 *
 * Why this file exists
 * --------------------
 * Every row used to hand `null` as its target element to
 * `makeDragOverHandler` / `makeDropHandler`. The zone math starts from
 * the row's bounding rect, so a null rect collapsed the split to a
 * constant `'above'` — with no error anywhere. Two gestures died
 * silently at once:
 *
 *   1. `into` became unreachable on container rows, so "drag into a
 *      module / collection" never reached the `moveNodes` branch.
 *   2. `below` became unreachable everywhere, so every reorder landed
 *      *above* its target — dragging A under B looked like no-op.
 *
 * Both are invisible to typecheck and to a passing build, so the rule
 * itself is pinned here: with a real rect, all three zones must be
 * reachable, and the split must match the row's nesting capability.
 *
 * Run via:
 *   npx tsx --test src/features/project-tree/hooks/__tests__/useTreeDnd.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { zoneFromRowPosition } from '../useTreeDnd';

// ---------- fixtures ----------

/** A 100px-tall row starting 200px down the viewport. */
const ROW = { top: 200, height: 100 };

/** Cursor Y for a given vertical ratio within `ROW`. */
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
    // 0.28 is the exclusive upper bound of the top quarter.
    assert.equal(zoneFromRowPosition(ROW, yAt(0.27), supportsInto), 'above');
  });

  it('answers into across the middle half — the "nest under it" band', () => {
    assert.equal(zoneFromRowPosition(ROW, yAt(0.28), supportsInto), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.5), supportsInto), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.72), supportsInto), 'into');
  });

  it('answers below at the bottom edge', () => {
    // 0.72 is the exclusive lower bound of the bottom quarter.
    assert.equal(zoneFromRowPosition(ROW, yAt(0.73), supportsInto), 'below');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.99), supportsInto), 'below');
  });

  it('reaches every zone, so neither nesting nor downward reorder can go missing', () => {
    // The regression this file exists for: a constant answer meant one
    // of these three was dead code no matter what the cursor did.
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
    // The container thresholds are 0.28 / 0.72; a leaf must not inherit
    // them, or a pointer resting mid-row would try to nest a node into
    // an endpoint.
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
    // height 0 would make the ratio ±Infinity / NaN, which satisfies no
    // comparison — the old inline math answered 'into' or 'below' from
    // whichever branch happened to swallow the NaN.
    assert.equal(zoneFromRowPosition({ top: 200, height: 0 }, 200, true), 'above');
    assert.equal(zoneFromRowPosition({ top: 200, height: 0 }, 200, false), 'above');
  });

  it('is viewport-relative, not page-relative', () => {
    // clientY is viewport coordinates, so the rect must be read as such:
    // a scrolled tree still splits at the same cursor heights.
    const scrolled = { top: 40, height: 100 };
    assert.equal(zoneFromRowPosition(scrolled, 90, true), 'into');
    assert.equal(zoneFromRowPosition(scrolled, 130, true), 'below');
  });
});
