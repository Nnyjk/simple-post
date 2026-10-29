/**
 * `{{变量}}` 提示的纯规则。
 *
 * 这个模块只做判定，不碰界面：找出用户正在输入的半个变量名、判断候选浮层该不该出现、
 * 把半个名字换成完整的 `{{key}}`、判断一次按键该做什么。浮层的位置在 `VarHintPicker`，
 * 写回在 `useVarHint` 与编辑器那一侧。
 */

/**
 * 候选列表里的一项。
 *
 * `origin` 区分环境变量与动态变量：两者并进同一份候选，用户要能一眼分出哪个是自动取回
 * 的、哪个是自己填的。
 */
export interface VarEntry {
  key: string;
  value: string;
  origin?: 'environment' | 'dynamic';
  /** 动态变量的取值来源与缓存年龄，供悬停时显示。 */
  hint?: string;
}

/**
 * 从文本里取出用户正在输入的半个变量名。
 *
 * 取的是最后一个 `{{` 之后、任何 `}}` 之前的文本。文本不在未闭合的 `{{…}}` 块里时返回
 * 空串——空串既表示「没在输入变量名」，也表示「刚敲下 `{{` 还没开始打字」，故判断浮层
 * 该不该出现要用另一个函数。
 */
export function extractPartialVar(text: string): string {
  const lastOpen = text.lastIndexOf('{{');
  if (lastOpen < 0) return '';
  const tail = text.slice(lastOpen + 2);
  if (tail.includes('}}')) return '';
  const match = tail.match(/^([^{}\s]*)/);
  return match ? match[1] : '';
}

/**
 * 文本是否正处在一个未闭合的 `{{…}}` 块里。
 *
 * 这决定候选浮层该不该出现：刚敲下 `{{` 那一刻还没有半个名字，但用户确实想要候选。
 */
export function isInsideVarBlock(text: string): boolean {
  const lastOpen = text.lastIndexOf('{{');
  if (lastOpen < 0) return false;
  return !text.slice(lastOpen + 2).includes('}}');
}

/**
 * 把文本末尾未闭合的 `{{片段` 换成 `{{key}}`。
 *
 * 只换末尾那一段：前面的 `{{` 可能早已闭合，换掉它会改坏用户已经写好的内容。
 */
export function replacePartialVar(text: string, key: string): string {
  return text.replace(/\{\{[^}\s]*$/, `{{${key}}}`);
}

/**
 * 按用户输入的片段过滤候选条目。
 *
 * 匹配用「包含」而非「前缀」：用户多半只记得变量名中间那一截。片段为空时返回全部候选。
 */
export function filterVarEntries(entries: VarEntry[], partial: string): VarEntry[] {
  const q = partial.toLowerCase();
  if (!q) return entries;
  return entries.filter((e) => e.key.toLowerCase().includes(q));
}

/** 一次按键该做什么。`none` 是「不归提示管」，按键按它本来的意思走。 */
export type VarHintKeyAction = 'none' | 'next' | 'prev' | 'insert' | 'close';

/** 判定按键要用到的那几项。两条路径的事件类型不同，故只取共同的几个。 */
export interface VarHintKeyEvent {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  /** 输入法正在组合。组合期间的 Enter 与方向键属于输入法候选，不是选变量。 */
  isComposing: boolean;
  /** 老浏览器上 `isComposing` 取不到时的兜底：229 是 IME 专用的 keyCode。 */
  keyCode: number;
}

/**
 * 这次按键该做什么。
 *
 * 两条路径（普通输入框与 CodeMirror 编辑器）吃的是不同类型的事件，规则却只有一份 ——
 * 判定拆成这个纯函数，两边各调它，故两边不会各自长出不同的行为。
 *
 * 三条不拦按键的情况：
 * - 浮层没开：这一栏根本没有候选可管。
 * - 输入法正在组合：组合期间的 Enter 与方向键属于输入法候选，拦下来会吞掉用户的中文。
 * - 没有候选可选：Enter 与 Tab 无从插起，拦下来键盘用户就出不去了。
 */
export function varHintKeyAction(
  event: VarHintKeyEvent,
  state: { open: boolean; hasEntry: boolean },
): VarHintKeyAction {
  if (!state.open) return 'none';
  if (event.isComposing || event.keyCode === 229) return 'none';
  // 放过带修饰键的组合，全局快捷键（Ctrl+Enter、Ctrl+S）才仍能触发。
  if (event.ctrlKey || event.metaKey || event.altKey) return 'none';
  if (event.key === 'ArrowDown') return 'next';
  if (event.key === 'ArrowUp') return 'prev';
  if (event.key === 'Enter' || event.key === 'Tab') return state.hasEntry ? 'insert' : 'none';
  if (event.key === 'Escape') return 'close';
  return 'none';
}
