import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

/** `micro`(11px) / `nano`(10px) 是 `tailwind.config.js` 里自定义的字号档，
 *  `tailwind-merge` 默认不认识 —— 不认识就当成「互相不冲突的类」，
 *  `cn('text-xs', 'text-micro')` 会把两个都留下，胜负由样式表顺序决定而不是参数顺序。
 *  登记进 `font-size` 组后，它就是普通字号，后写的照样压过前面的。 */
const twMerge = extendTailwindMerge({
  extend: { classGroups: { 'font-size': [{ text: ['micro', 'nano'] }] } },
});

/** 条件 className 拼接 —— 先用 `clsx` 合并真值入参，再用 `tailwind-merge`
 *  让后面的工具类压过前面的（如 `cn('px-4', false && 'px-2')` → `'px-4'`）。 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * 应用里交互元素唯一的焦点环写法，当 `cn()` 的一个参数用：`cn('…基础样式', focusRing)`。
 *
 * 焦点环的宽度与偏移只在这里定义，各控件从 `focusRing` 取。
 */
export const focusRing =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ' +
  'focus-visible:ring-offset-1 focus-visible:ring-offset-background';

/**
 * 同样的环，但无偏移。给贴着窗口边缘的自定义标题栏控件（`WindowControls`）与权限单选
 * （`AgentPermissionsTable`）用——那里 1px 偏移会被裁掉。单独成一个常量而不是就地内联，
 * 是为了让「贴边」与「内嵌」的差异保持有意且可 grep：某个控件若哪天从贴边改成内嵌，
 * 换的是一个 token。
 */
export const focusRingBare =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';

/**
 * 同上，但环朝内。给被父容器 `overflow-hidden` 裁掉外侧的控件用 —— 目前只有标签条的
 * 标签（条高 36px、标签 `h-full`，外侧的 2px 环连同 1px 偏移都会被条裁没）。
 */
export const focusRingInset =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset';

/**
 * 行内小图标按钮（24×24、只有图标、`aria-label` 取名）的唯一写法：胶囊行尾的刷新 / 详情 / 删除、
 * 命令面板的关闭。
 * 尺寸固定 `h-6 w-6`：这些按钮嵌在 30 来像素高的行里，再大就把行撑开。
 * 用的时候 `cn(iconButton, '…')`，`focusRing` 已经含在里面。
 */
export const iconButton = cn(
  'inline-flex h-6 w-6 items-center justify-center rounded text-muted-foreground transition-colors hover:text-foreground disabled:pointer-events-none disabled:opacity-40',
  focusRing,
);

export function formatBytes(bytes: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let n = bytes;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1) return '< 1 ms';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function methodColorVar(method: string): string {
  switch (method.toUpperCase()) {
    case 'GET':
      return 'hsl(142 71% 45%)';
    // POST 与 PUT 刻意与常见 REST 配色对调：产品里别处蓝色留给「创建」（POST），
    // 所以 POST 取更暖的橙、PUT 取更冷的蓝，两个创建/更新动词并排时不会糊在一起
    case 'POST':
      return 'hsl(38 92% 50%)';
    case 'PUT':
      return 'hsl(217 91% 60%)';
    case 'PATCH':
      return 'hsl(280 65% 60%)';
    case 'DELETE':
      return 'hsl(0 72% 51%)';
    default:
      return 'hsl(220 14% 46%)';
  }
}

export function statusColorVar(status: number): string {
  if (status >= 500) return 'hsl(0 72% 51%)';
  if (status >= 400) return 'hsl(38 92% 50%)';
  if (status >= 300) return 'hsl(217 91% 60%)';
  if (status >= 200) return 'hsl(142 71% 45%)';
  return 'hsl(220 14% 46%)';
}

/**
 * 状态码的**文字**色档。`statusColorVar` 是块色（给图标用），文字不能跟着用：那些色相
 * 不随主题变化，浅色 card 上绿字只有 2.3:1。`-foreground` 才是主题换极性的文字档。
 */
export function statusTextClass(status: number): string {
  if (status >= 500) return 'text-destructive';
  if (status >= 400) return 'text-warning-foreground';
  if (status >= 300) return 'text-muted-foreground';
  if (status >= 200) return 'text-success-foreground';
  return 'text-muted-foreground';
}

/**
 * 固定色相实色底上的可读文字色。`methodColorVar` 这类色相不随主题变化，因此配不上会换
 * 极性的 `-foreground` token；改为按 WCAG 相对亮度在近黑/白之间选对比度更高的那个
 * （`hsl(38 92% 50%)` 上白字 2.1:1、近黑字 9.8:1）。解析失败时退回白字。
 */
export function readableInk(hslColor: string): string {
  const m = /hsl\(([\d.]+)\s+([\d.]+)%\s+([\d.]+)%\)/.exec(hslColor);
  if (!m) return 'hsl(0 0% 100%)';
  const h = Number(m[1]) / 360;
  const s = Number(m[2]) / 100;
  const l = Number(m[3]) / 100;
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const channel = (t: number) => {
    const x = t < 0 ? t + 1 : t > 1 ? t - 1 : t;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  const linear = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const luminance =
    0.2126 * linear(channel(h + 1 / 3)) +
    0.7152 * linear(channel(h)) +
    0.0722 * linear(channel(h - 1 / 3));
  return luminance > 0.179 ? 'hsl(0 0% 10%)' : 'hsl(0 0% 100%)';
}

export function uid(): string {
  return Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
}

// ---------------------------------------------------------------------------
// JSON 美化 —— 响应查看器与 body 编辑器的「格式化」按钮共用。返回带标签的结果，调用方
// 无需 try/catch 就能区分成功与解析错误。放在 `utils.ts` 是因为 `tryFormatJson` 纯且无依赖
// （`useIsDark` 会 import `useAppStore`，因此住在 `theme.ts`，以免形成
// `crud-slice → utils → app-store` 的环）。
// ---------------------------------------------------------------------------

export type FormatResult = { ok: true; formatted: string } | { ok: false; error: string };

export function tryFormatJson(input: string, indent = 2): FormatResult {
  const s = input.trim();
  if (!s) return { ok: false, error: '内容为空' };
  try {
    const parsed = JSON.parse(s);
    return { ok: true, formatted: JSON.stringify(parsed, null, indent) };
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }
}
