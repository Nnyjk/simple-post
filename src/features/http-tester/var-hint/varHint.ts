/**
 * `{{变量}}` 提示的纯文本规则。
 *
 * 这个模块只处理字符串：找出用户正在输入的半个变量名、判断候选浮层该不该出现、把半个
 * 名字换成完整的 `{{key}}`。浮层的位置在 `VarHintPicker`，写回与键盘导航在 `useVarHint`。
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
