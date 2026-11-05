/**
 * 标签条横向滚动的目标位置计算。
 *
 * 为什么单独一个文件：这条规则要能被单测钉住，而它与 GSAP、与 React 都无关 —— 它只读
 * 两个矩形，算出一个数字。放在组件里就只能靠界面验证覆盖它。
 *
 * 背景：标签条是 `overflow-hidden` 的横向滚动容器（不出现滚动条，但可以滚动）。标签多到
 * 一条放不下时，激活标签可能落在可视范围之外。原生 `scrollIntoView` 会**瞬间**改掉
 * `scrollLeft`，于是所有标签的视口位置在同一帧里挪一截 —— 那一下不参与任何动画，读起来
 * 就是「标签条猛地跳一下」。故滚动由 GSAP 补间完成，这个函数只负责算出该滚到哪儿。
 */

/** 条与标签各自只需要左右边界。 */
export interface HorizontalBox {
  readonly left: number;
  readonly right: number;
}

/** 承载滚动的标签条。 */
export interface ScrollHost {
  readonly scrollLeft: number;
  readonly scrollWidth: number;
  readonly clientWidth: number;
  getBoundingClientRect(): HorizontalBox;
}

/**
 * 让激活标签落进条的可视范围，算出该滚到哪儿。
 *
 * 规则三条：
 *
 * 1. 内容放得下（`scrollWidth <= clientWidth`）时不需要滚，返回 null。
 * 2. 目标标签在可视范围之外才滚；已经在里面就保持原位 —— 已经在里面的标签被重新滚一遍
 *    会让整条标签平移一截，看起来像被推了一下。
 * 3. 滚到的位置夹在 `[0, scrollWidth - clientWidth]` 之间，故标签不会被滚出条外。
 *
 * 返回 null 表示「不需要滚」。
 */
export function scrollTargetForActiveTab(host: ScrollHost, target: HorizontalBox): number | null {
  const maxScroll = host.scrollWidth - host.clientWidth;
  if (maxScroll <= 0) return null;

  const hostRect = host.getBoundingClientRect();
  let next = host.scrollLeft;
  if (target.left < hostRect.left) {
    // 目标在左边：把它的左缘滚到条的左缘。
    next = host.scrollLeft + (target.left - hostRect.left);
  } else if (target.right > hostRect.right) {
    // 目标在右边：把它的右缘滚到条的右缘。
    next = host.scrollLeft + (target.right - hostRect.right);
  }

  next = Math.min(Math.max(next, 0), maxScroll);
  if (Math.abs(next - host.scrollLeft) < 0.5) return null;
  return next;
}
