/**
 * 锚点的屏幕矩形。
 *
 * 只取四个边：`DOMRect` 有它们，CodeMirror 的 `coordsAtPos` 给的也是它们，故两种锚点
 * 共用同一个形状。宽度由 `right - left` 算出，故不必另带一个 `width` 字段。
 */
export interface VarHintRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/**
 * 候选浮层的定位锚点。
 *
 * 锚点是两个动作而不是一个矩形：定位要知道矩形在哪，「点击浮层外面就关闭」要知道那次
 * 点击落没落在输入区域里。
 */
export interface VarHintAnchor {
  /** 锚点的屏幕坐标。返回 `null` 时浮层不显示。 */
  getRect: () => VarHintRect | null;
  /** 判断一次点击是否落在锚点范围内。 */
  contains: (target: Node) => boolean;
}

/**
 * 锚在一个 DOM 元素上。
 *
 * `getRect` 与 `contains` 每次调用都重新读那个元素，故元素挪了位置或换了内容都不必重建
 * 锚点。
 */
export function elementAnchor(el: HTMLElement | null): VarHintAnchor {
  return {
    getRect: () => el?.getBoundingClientRect() ?? null,
    contains: (target) => el?.contains(target) ?? false,
  };
}

/**
 * 锚在一个 ref 持有的元素上。
 *
 * 元素在首帧渲染之前还不存在，故锚点读的是 ref 的当前值，而不是渲染那一刻的值。
 */
export function refAnchor(ref: { current: HTMLElement | null }): VarHintAnchor {
  return {
    getRect: () => ref.current?.getBoundingClientRect() ?? null,
    contains: (target) => ref.current?.contains(target) ?? false,
  };
}

/**
 * 锚在一个随光标移动的矩形上。
 *
 * CodeMirror 的编辑器里没有「光标所在的那个输入框」，故由调用方按光标位置算出矩形。
 * `contains` 缺省为「永不包含」：编辑器是一大片区域，把整块编辑器算成锚点会让点击
 * 编辑器里的别处也不关闭浮层。
 */
export function rectAnchor(
  getRect: () => VarHintRect | null,
  contains: (target: Node) => boolean = () => false,
): VarHintAnchor {
  return { getRect, contains };
}
