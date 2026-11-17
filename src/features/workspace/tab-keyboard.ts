/**
 * 标签条的方向键落点。纯函数而非写在 `WorkspaceTabs` 的 `onKeyDown` 里：那条路径要读
 * DOM（`querySelectorAll`、`focus`），而这段「下一个是哪个」不需要，单测因此不必造浏览器。
 *
 * 返回 `-1` 表示这个键不归标签条管（调用方原样放行，避免吞掉页面的其它快捷键）。
 * `Home`/`End` 是 ARIA tabs 模式的标配；越界一律环绕，标签条是循环的。
 */
export function nextTabFocusIndex(key: string, current: number, count: number): number {
  if (count <= 0 || current < 0) return -1;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  if (key === 'ArrowRight') return (current + 1) % count;
  if (key === 'ArrowLeft') return (current - 1 + count) % count;
  return -1;
}
