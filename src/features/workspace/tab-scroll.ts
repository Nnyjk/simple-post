/**
 * 标签条横向滚动的目标位置与滚动条几何。
 *
 * 为什么单独一个文件：这两条规则要能被单测钉住，而它们与 GSAP、与 React 都无关 ——
 * 一个只读两个矩形算出一个数字，另一个只读三个尺寸算出一个盒子。放在组件里就只能靠
 * 界面验证覆盖它们。
 *
 * 背景：标签条是 `overflow-hidden` 的横向滚动容器（原生滚动条被关掉，改由一条覆盖式的
 * 细滚动条呈现）。标签多到一条放不下时，激活标签可能落在可视范围之外。原生
 * `scrollIntoView` 会**瞬间**改掉 `scrollLeft`，于是所有标签的视口位置在同一帧里挪一
 * 截 —— 那一下不参与任何动画，读起来就是「标签条猛地跳一下」。故滚动由 GSAP 补间完成，
 * 这个文件只负责算出该滚到哪儿、以及滑块该画在哪儿。
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
  // 差不足半个像素就不滚：浏览器给出的 rect 带小数，差这么点滚过去看不出区别，
  // 却会让滚动位置变成一个带小数的数。
  if (Math.abs(next - host.scrollLeft) < 0.5) return null;
  return next;
}

/** 滚动条滑块的盒子：内容坐标里的左缘与宽度。 */
export interface ThumbBox {
  readonly left: number;
  readonly width: number;
}

/**
 * 算出覆盖式滚动条滑块该画在哪儿。
 *
 * 契约（逐条）：
 *
 * 1. `maxScroll = scrollWidth - clientWidth`；`maxScroll <= 0.5` → 返回 null ——
 *    内容放得下时**不显示滚动条**（返回 null 表示「没有滑块」）。
 * 2. `width = clamp(clientWidth * clientWidth / scrollWidth, minThumbPx, clientWidth)`。
 *    比例式宽度：内容越宽，滑块越短；滑块至少 `minThumbPx`（默认 24px）宽，且永远
 *    不超过可视宽度。
 * 3. `left = (scrollLeft / maxScroll) * (clientWidth - width)`，再夹到
 *    `[0, clientWidth - width]`：滑块不会滑出轨道，`scrollLeft` 越界（回弹、程序
 *    赋值）也夹得住。
 *
 * 只用内容坐标（`scrollWidth` / `clientWidth` / `scrollLeft`），**不使用**
 * `getBoundingClientRect` —— 滑块的位置与滚动目标无关，只跟内容有多长、滚到哪儿有关。
 */
export function scrollbarThumb(host: ScrollHost, minThumbPx = 24): ThumbBox | null {
  const maxScroll = host.scrollWidth - host.clientWidth;
  if (maxScroll <= 0.5) return null;

  const width = Math.min(
    Math.max((host.clientWidth * host.clientWidth) / host.scrollWidth, minThumbPx),
    host.clientWidth,
  );
  const left = Math.min(
    Math.max((host.scrollLeft / maxScroll) * (host.clientWidth - width), 0),
    host.clientWidth - width,
  );
  return { left, width };
}
