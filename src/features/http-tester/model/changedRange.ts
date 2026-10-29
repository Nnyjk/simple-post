/**
 * 两段文本之间真正变了的那一段。
 *
 * 用来只替换差异部分而不是整篇覆盖：整篇覆盖会让编辑器重建语法树，折叠状态与选区
 * 一并丢掉，而用户只是往里插了一个变量。
 */

/** 差异的位置与内容。 */
export interface ChangedRange {
  /** 差异在旧文本里的起点。 */
  from: number;
  /** 差异在旧文本里的终点（不含）。 */
  to: number;
  /** 换进去的那一段。 */
  insert: string;
}

/**
 * 找出 `prev` 变成 `next` 需要动的那一段。
 *
 * 做法是先把两段文本的公共前缀与公共后缀剥掉，剩下的一段就是要替换的。纯插入
 * （`prev` 是 `next` 的前缀）算出来的是空区间，`to === from`。
 *
 * 下标是 UTF-16 码元，与 CodeMirror 的文档下标一致。
 */
export function changedRange(prev: string, next: string): ChangedRange {
  let from = 0;
  while (from < prev.length && from < next.length && prev[from] === next[from]) from++;
  let endPrev = prev.length;
  let endNext = next.length;
  while (endPrev > from && endNext > from && prev[endPrev - 1] === next[endNext - 1]) {
    endPrev--;
    endNext--;
  }
  return { from, to: endPrev, insert: next.slice(from, endNext) };
}
