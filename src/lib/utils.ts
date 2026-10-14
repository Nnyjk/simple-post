import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

/** 条件 className 拼接 —— 先用 `clsx` 合并真值入参，再用 `tailwind-merge`
 *  让后面的工具类压过前面的（如 `cn('px-4', false && 'px-2')` → `'px-4'`）。 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * 应用里交互元素唯一的焦点环写法，当 `cn()` 的一个参数用：`cn('…基础样式', focusRing)`。
 *
 * 只留一份意味着日后改环的宽度或偏移时处处同时生效——手抄的字面量正是那种会
 * 悄悄漂移、一半控件 1px 一半 2px 的东西。
 */
export const focusRing =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ' +
  'focus-visible:ring-offset-1 focus-visible:ring-offset-background';

/**
 * 同样的环，但无偏移。给贴着窗口边缘的自定义标题栏控件（`WindowControls`）与
 * 权限单选（`AgentPermissionsTable`）用——那里 1px 偏移会被裁掉。单独成一个常量
 * 而不是就地内联，是为了让「贴边」与「内嵌」的差异保持有意且可 grep：某个控件若哪天
 * 从贴边改成内嵌，换的是一个 token。
 */
export const focusRingBare =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';

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

export function uid(): string {
  return Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
}

// ---------------------------------------------------------------------------
// JSON 美化 —— 响应查看器与 body 编辑器的「格式化」按钮共用。返回带标签的结果，
// 调用方无需 try/catch 就能区分成功与解析错误。
//
// 放在 `utils.ts` 是因为 `tryFormatJson` 纯且无依赖（`useIsDark` 会 import
// `useAppStore`，因此住在 `theme.ts`，以免形成 `crud-slice → utils → app-store`
// 的环）。
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
