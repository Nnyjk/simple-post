/**
 * 项目强调色 —— 新建项目时可选的调色板，由这一份列表独占。
 *
 * 这些色值在明暗两种卡片底色上都可读，并与资源树使用的语义 `hue` 强调色（见
 * `src/lib/hue.ts`）保持距离——项目自身的颜色应当读作装饰，而不是节点类型。
 *
 * `PROJECT_COLOR_DEFAULT` 同时也是全新项目被写入的 `color`，所以 store 从这里
 * import 是安全的：`ui-slice` 本就依赖本模块，而本模块不 import store。
 */
export const PROJECT_COLORS = [
  '#60a5fa', // blue
  '#a78bfa', // violet
  '#34d399', // emerald
  '#fbbf24', // amber
  '#f87171', // red
  '#a3a3a3', // neutral
] as const;

/** 新建项目的种子色 —— 第一块色板。 */
export const PROJECT_COLOR_DEFAULT: string = PROJECT_COLORS[0];
