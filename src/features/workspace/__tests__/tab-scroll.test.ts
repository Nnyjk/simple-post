/**
 * `src/features/workspace/tab-scroll.ts` 的纯单测 —— 标签条该滚到哪儿。
 *
 * 这条规则以前住在组件里（原生 `scrollIntoView`），它的问题是**瞬间**改掉 `scrollLeft`：
 * 所有标签的视口位置在同一帧里挪一截，那一下不参与任何动画，读起来就是「标签条
 * 猛地跳一下」。改成 GSAP 补间之后，「该滚到哪儿」就成了可以单测的纯计算。
 *
 * 运行：npx tsx --test src/features/workspace/__tests__/tab-scroll.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { scrollTargetForActiveTab, scrollbarThumb, type ScrollHost } from '../tab-scroll';

/** 造一个标签条：可视宽度 `clientWidth`，内容宽度 `scrollWidth`，当前滚到 `scrollLeft`。 */
function makeHost(opts: {
  scrollLeft?: number;
  scrollWidth: number;
  clientWidth: number;
  left?: number;
}): ScrollHost {
  const left = opts.left ?? 256;
  return {
    scrollLeft: opts.scrollLeft ?? 0,
    scrollWidth: opts.scrollWidth,
    clientWidth: opts.clientWidth,
    getBoundingClientRect: () => ({ left, right: left + opts.clientWidth }),
  };
}

describe('scrollTargetForActiveTab', () => {
  it('内容放得下时不滚', () => {
    const host = makeHost({ scrollWidth: 800, clientWidth: 1000 });
    assert.equal(scrollTargetForActiveTab(host, { left: 300, right: 380 }), null);
  });

  it('目标已经在可视范围内时不滚', () => {
    const host = makeHost({ scrollLeft: 200, scrollWidth: 2000, clientWidth: 1000 });
    // 目标落在条的可视范围内（条的左缘 256、右缘 1256）。
    assert.equal(scrollTargetForActiveTab(host, { left: 400, right: 500 }), null);
  });

  it('目标在左边时把它的左缘滚到条的左缘', () => {
    const host = makeHost({ scrollLeft: 400, scrollWidth: 2000, clientWidth: 1000 });
    // 目标左缘 200，条的左缘 256 → 要往左滚 56。
    assert.equal(scrollTargetForActiveTab(host, { left: 200, right: 300 }), 344);
  });

  it('目标在右边时把它的右缘滚到条的右缘', () => {
    const host = makeHost({ scrollLeft: 100, scrollWidth: 2000, clientWidth: 1000 });
    // 目标右缘 1400，条的右缘 1256 → 要往右滚 144。
    assert.equal(scrollTargetForActiveTab(host, { left: 1300, right: 1400 }), 244);
  });

  it('滚到的位置不超过可滚动的上限', () => {
    const host = makeHost({ scrollLeft: 0, scrollWidth: 1200, clientWidth: 1000 });
    // 上限是 200；目标右缘超出条很多，算出来的值必须夹到 200。
    assert.equal(scrollTargetForActiveTab(host, { left: 1500, right: 1600 }), 200);
  });

  it('滚到的位置不小于零', () => {
    const host = makeHost({ scrollLeft: 30, scrollWidth: 2000, clientWidth: 1000 });
    // 目标左缘 200，条的左缘 256 → 算出来是 -26，必须夹到 0。
    assert.equal(scrollTargetForActiveTab(host, { left: 200, right: 300 }), 0);
  });

  it('已经在目标位置时不再滚', () => {
    const host = makeHost({ scrollLeft: 344, scrollWidth: 2000, clientWidth: 1000 });
    // 目标的左缘已经贴着条的左缘（256），算出来的目标位置与当前滚动位置相同 → 不滚。
    assert.equal(scrollTargetForActiveTab(host, { left: 256, right: 300 }), null);
  });

  it('差不足半个像素时也不滚', () => {
    const host = makeHost({ scrollLeft: 200, scrollWidth: 2000, clientWidth: 1000 });
    // 目标左缘 255.75，条的左缘 256 → 只差 0.25px，不滚。
    assert.equal(scrollTargetForActiveTab(host, { left: 255.75, right: 300 }), null);
  });
});

describe('scrollbarThumb', () => {
  it('内容放得下时不显示滚动条', () => {
    const host = makeHost({ scrollWidth: 800, clientWidth: 1000 });
    assert.equal(scrollbarThumb(host), null);
  });

  it('滑块宽度与可视宽度成正比', () => {
    // 可视 1000、内容 2000 → 滑块占一半。
    const host = makeHost({ scrollWidth: 2000, clientWidth: 1000 });
    assert.equal(scrollbarThumb(host)?.width, 500);
  });

  it('滑块不短于最小宽度', () => {
    // 可视 1000、内容 100000 → 比例宽只有 10px，按最小宽度 24 兜底。
    const host = makeHost({ scrollWidth: 100000, clientWidth: 1000 });
    assert.equal(scrollbarThumb(host)?.width, 24);
  });

  it('滑块宽度不超过可视宽度', () => {
    // 内容只比可视宽 1px：比例宽接近可视宽，仍不得超过它。
    const host = makeHost({ scrollWidth: 1001, clientWidth: 1000 });
    const box = scrollbarThumb(host);
    assert.ok(box);
    assert.ok(box.width <= 1000);
  });

  it('滚到最右时滑块贴住轨道的右端', () => {
    const host = makeHost({ scrollLeft: 1000, scrollWidth: 2000, clientWidth: 1000 });
    const box = scrollbarThumb(host);
    assert.ok(box);
    assert.equal(box.left, 1000 - box.width);
  });

  it('滚到最左时滑块贴住轨道的左端', () => {
    const host = makeHost({ scrollLeft: 0, scrollWidth: 2000, clientWidth: 1000 });
    assert.equal(scrollbarThumb(host)?.left, 0);
  });

  it('scrollLeft 越界时滑块被夹在轨道内', () => {
    const host = makeHost({ scrollLeft: 9999, scrollWidth: 2000, clientWidth: 1000 });
    const box = scrollbarThumb(host);
    assert.ok(box);
    assert.equal(box.left, 1000 - box.width);
  });
});
