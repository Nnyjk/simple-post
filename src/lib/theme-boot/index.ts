/**
 * 偏好键与主题解析的唯一来源。
 *
 * 三处用到本模块：
 *
 * - `index.html` 的 `<head>` 里那段同步脚本。它由 `vite.config.ts` 里的插件在**构建期**
 *   从本文件的源码生成（见同目录的 `plugin.ts`），故本文件是那段脚本的唯一来源。
 * - `src/stores/slices/ui-prefs.ts`。store 读偏好、写偏好都用本文件提供的键与解析函数。
 *
 * `src/main.tsx` 不再自己解析主题：`App.tsx` 的挂载副作用本来就会把主题写到 `<html>` 上，
 * 而构建期注入的那段脚本在 HTML 解析期就做完了同一件事，比 React 挂载更早。
 *
 * **本文件的运行时不能有任何 import 语句。** 内联脚本是 classic script，浏览器不允许它
 * import 别的模块。故本文件不引入任何运行时依赖，类型也只在本文件里就地定义。
 *
 * **本文件的运行时只能有一个默认导出。** 插件把本文件转成 JavaScript 之后，把那一行默认
 * 导出改写成 `globalThis.__simplePostTheme =`（内联脚本拿不到模块系统的导出）。多一个导出
 * 或多一条 import，插件会在构建期报错，而不是让注入的脚本少东西。
 */

/** 视觉主题预设。驱动 document 根节点上的 CSS 变量。 */
export type Theme = 'dark' | 'light' | 'system';

/**
 * localStorage 里存的那份偏好。
 *
 * `theme` 存的是**未经校验**的原始取值：用户可以手改 localStorage，也可能留着旧版本写的
 * 值。什么算合法主题由 [`resolveTheme`] 判定，不由这份类型断言。
 */
export interface PersistedPrefs {
  theme?: string;
  /** 用户上次停留的项目 id。空串表示「不在任何项目里」。 */
  lastProjectId?: string;
  /** 旧版本把请求旋钮存在这里。升级后它们在 `preferences` 表里，故只用于一次性迁移。 */
  requestSettings?: unknown;
}

/** 偏好所在的 localStorage 键。整个仓库只有这一处写这个字符串。 */
const PREFS_KEY = 'simple-post:prefs';

/**
 * 解析一份 localStorage 里的偏好原文。
 *
 * 解析失败一律当作「什么都没有」：localStorage 里的内容可能被人手改坏，应用不该因此
 * 起不来。数组与 `null` 同样当作空对象——它们不是偏好的形状。
 */
function parsePrefs(raw: string | null): PersistedPrefs {
  if (typeof raw !== 'string' || raw === '') return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
  return parsed as PersistedPrefs;
}

/**
 * 判定一个存下来的主题取值算不算合法主题。
 *
 * 只有 `dark` / `light` / `system` 三个取值合法，其余（空串、`null`、旧版本留下的别的名字、
 * 手改坏的值）一律回落到 `dark`——那是应用的缺省主题。
 *
 * **校验放在这里而不是各处各判一次**，是因为三处（构建期注入的脚本、store 的初值、
 * 界面读当前主题）必须对同一个取值给出同一个答案：首帧画深色、随后 React 画系统色，
 * 那种闪烁是分歧的直接后果。
 */
function resolveTheme(value: unknown): Theme {
  return value === 'dark' || value === 'light' || value === 'system' ? value : 'dark';
}

/**
 * 操作系统是不是深色。
 *
 * 浏览器可能没有 `matchMedia`（测试环境、极老的浏览器），此时按浅色处理。
 */
function systemPrefersDark(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  try {
    return window.matchMedia('(prefers-color-scheme: dark)').matches;
  } catch {
    return false;
  }
}

/**
 * 这个主题要不要画成深色。
 *
 * `system` 交给操作系统，其余两个固定值直接给答案。
 */
function prefersDark(theme: Theme): boolean {
  if (theme === 'dark') return true;
  if (theme === 'light') return false;
  return systemPrefersDark();
}

/**
 * 读出 localStorage 里存的那份偏好。
 *
 * localStorage 可能被禁用（隐私模式、配额用尽），读不到时当作没有偏好。
 */
function readPersistedPrefs(): PersistedPrefs {
  if (typeof window === 'undefined') return {};
  try {
    return parsePrefs(window.localStorage.getItem(PREFS_KEY));
  } catch {
    return {};
  }
}

/**
 * 读出当前生效的主题。
 *
 * 这是「读偏好 + 校验」两步合起来的结果，与构建期注入的脚本用的是同一套判断。
 */
function readPersistedTheme(): Theme {
  return resolveTheme(readPersistedPrefs().theme);
}

/**
 * 把当前主题写到 `<html>` 上。
 *
 * 调 `classList.toggle('dark', …)` 而不是 `add` / `remove`：切主题时同一个调用把两个方向
 * 都管住，不会漏掉「从深色切回浅色」那一半。
 *
 * 这段代码会被原样注入 `index.html` 的 `<head>`，因此它只依赖 `localStorage`、
 * `matchMedia` 与 `document.documentElement`——三者此刻都已就绪。
 */
function applyInitialTheme(): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  if (!root) return;
  root.classList.toggle('dark', prefersDark(readPersistedTheme()));
}

export default {
  PREFS_KEY,
  parsePrefs,
  resolveTheme,
  prefersDark,
  readPersistedPrefs,
  readPersistedTheme,
  applyInitialTheme,
};
