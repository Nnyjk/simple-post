/**
 * 树的开合动效里与 DOM 无关的那几个数：批量窗口、逐行淡入的错峰计划。
 *
 * 纯模块，不 import React —— 写入方是 `ProjectTree` 的两个批量按钮，读取方是每个
 * `TreeChildren`，两者之间不该只为一个时间戳建立组件依赖。
 */

/**
 * 批量窗口：一次 `set()` 引发的重渲染与随后的 layout effect 都落在同一帧，120ms 足够宽。
 *
 * 窗口内挂载 / 变化的容器直接落到终态 —— 「全部展开 / 全部收起」让整棵树的展开状态在
 * 同一个依赖变化里一起变，若每个容器都补间自己的 `height`，一帧里就是几百个补间，
 * 每帧都要重新布局整棵子树。
 */
export const BULK_EXPAND_WINDOW_MS = 120;

let bulkMarkedAt = Number.NEGATIVE_INFINITY;

/** 记录一次批量展开 / 收起。`now` 只为测试注入时钟，调用方一律省略。 */
export function markBulkExpand(now = performance.now()): void {
  bulkMarkedAt = now;
}

/** 刚发生过批量动作？为真时容器直落终态，不补间。 */
export function isBulkExpandFresh(now = performance.now()): boolean {
  return now - bulkMarkedAt <= BULK_EXPAND_WINDOW_MS;
}

/** 展开时逐行淡入只排前几行：行数是数据量，不该变成动画时长。 */
export const REVEAL_MAX_ROWS = 8;
/** 相邻两行的开始间隔。 */
export const REVEAL_EACH = 0.015;
/** 整组错峰的总时长上限。 */
export const REVEAL_AMOUNT = 0.08;

export interface RevealPlan {
  /** 参与淡入的行数，最多 `REVEAL_MAX_ROWS`。 */
  rows: number;
  /** 相邻两行开始时间的间隔。 */
  each: number;
  /** 整组错峰的总时长，不随行数增长。 */
  amount: number;
}

/**
 * 展开时的错峰计划：行数再多也只排前几行，总时长封顶。
 *
 * 结果直接喂给 GSAP 的 `stagger: { each, amount }` —— `amount` 是总时长，故一个 200 行
 * 的集合与一个 8 行的集合淡入耗时相同。
 */
export function revealPlan(rowCount: number): RevealPlan {
  const rows = Math.max(0, Math.min(Math.floor(rowCount), REVEAL_MAX_ROWS));
  const span = Math.max(0, rows - 1) * REVEAL_EACH;
  return { rows, each: REVEAL_EACH, amount: Math.min(REVEAL_AMOUNT, span) };
}
