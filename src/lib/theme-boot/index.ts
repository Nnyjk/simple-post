/**
 * 偏好键与主题解析的唯一来源。
 *
 * 用它的两处：`index.html` 的首帧脚本（由 `plugin.ts` 在构建期从本文件的源码生成）与
 * `src/stores/slices/ui-prefs.ts`（store 读偏好、写偏好都用本文件提供的键与解析函数）。
 *
 * 两条构建期约束：运行时**不能有任何 import**（内联脚本是 classic script，浏览器不允许它
 * import），运行时**只能有一个默认导出**（`plugin.ts` 把它改写成给全局对象赋值）。多一条
 * import 或多一个导出，`plugin.ts` 在构建期报错，而不是让注入的脚本少一段东西。
 *
 * `src/main.tsx` 不解析初始主题：构建期注入的那段脚本在 HTML 解析期就把主题写到 `<html>`
 * 上，比 React 挂载更早。
 */

/** 视觉主题预设。驱动 document 根节点上的 CSS 变量。 */
export type Theme = 'dark' | 'light' | 'system';

/**
 * localStorage 里存的那份偏好。
 *
 * `theme` 存的是**未经校验**的原始取值：用户可以手改 localStorage，存进来的名字可能
 * 不在合法集合里。什么算合法主题由 [`resolveTheme`] 判定，不由这份类型断言。
 */
export interface PersistedPrefs {
  theme?: string;
  /**
   * 用户上次停留的项目 id。空串表示「不在任何项目里」。
   *
   * [`parsePrefs`] 保证它是字符串或不存在。
   */
  lastProjectId?: string;
  /** 只为一次性迁移保留：请求旋钮现在存在 `preferences` 表里，迁移成功后这个字段从 localStorage 删除。 */
  requestSettings?: unknown;
}

/** 偏好所在的 localStorage 键。整个仓库只有这一处写这个字符串。 */
const PREFS_KEY = 'simple-post:prefs';

/**
 * 解析一份 localStorage 里的偏好原文。
 *
 * 解析失败一律当作「什么都没有」：localStorage 里的内容可能被人手改坏，应用不该因此
 * 起不来。数组与 `null` 同样当作空对象——它们不是偏好的形状。
 *
 * `lastProjectId` 另有一条检查：它的值会被当作项目 id 拿去比对，非字符串的值直接丢掉。
 * `as` 只是类型断言，运行时给不出任何保证——不检查的话，读它的调用方拿到的就是一个
 * 类型上是 `string`、运行时是数字的值。`theme` 不在这里丢：它存的是原始取值，合不合法
 * 由 [`resolveTheme`] 判定。
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
  const prefs = parsed as PersistedPrefs;
  if (typeof prefs.lastProjectId !== 'string') delete prefs.lastProjectId;
  return prefs;
}

/**
 * 判定一个存下来的主题取值算不算合法主题。
 *
 * 只有 `dark` / `light` / `system` 三个取值合法，其余（空串、`null`、任何不在这三个里的
 * 名字、手改坏的值）一律回落到 `dark`——那是应用的缺省主题。
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
