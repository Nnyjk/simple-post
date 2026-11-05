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

import { scrollTargetForActiveTab, type ScrollHost } from '../tab-scroll';

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
});
