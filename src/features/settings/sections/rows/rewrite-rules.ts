/**
 * 有序路径改写规则的纯逻辑：拖拽落点、换序、可持久化的那一份、以及两份列表的等价判定。
 *
 * 本仓没有组件测试（`npm test` 只收集 `src` 下的 .test.ts），故凡是能写错的判定都放在这里，
 * 由 `__tests__/rewrite-rules.test.ts` 直接断言；`ModuleBaseUrlTable` 只负责把结果写进草稿。
 */
import type { RewriteRule } from '@/types/domain';

/** 拖到目标行的上半区还是下半区。上半区 = 插到它前面，下半区 = 插到它后面。 */
export type RewriteDropZone = 'above' | 'below';

/**
 * 由指针的纵坐标算出落在目标行的哪一半。`rect` 是那一行的测量值，由调用方传
 * （`getBoundingClientRect()` 的 `top` 与 `height`），故本函数不碰 DOM。
 */
export function resolveRewriteDropZone(
  clientY: number,
  rect: { top: number; height: number },
): RewriteDropZone {
  return clientY < rect.top + rect.height / 2 ? 'above' : 'below';
}

/**
 * 把第 `fromIndex` 条规则移到 `targetIndex` 那条的上面或下面。
 *
 * 边界无效（下标越界、`fromIndex === targetIndex`）或落点没有变化时返回**原数组引用**，
 * 调用方据此跳过一次空写。否则返回新数组，原数组不改。
 */
export function reorderRewriteRules(
  rules: readonly RewriteRule[],
  fromIndex: number,
  targetIndex: number,
  zone: RewriteDropZone,
): RewriteRule[] {
  const size = rules.length;
  const inRange = (i: number) => Number.isInteger(i) && i >= 0 && i < size;
  if (!inRange(fromIndex) || !inRange(targetIndex) || fromIndex === targetIndex) {
    return rules as RewriteRule[];
  }
  const rest = [...rules];
  const [moved] = rest.splice(fromIndex, 1);
  // `targetIndex` 指的是原数组里的那一行：移除之后它左边少了一项时下标要减一。
  const targetAfterRemoval = targetIndex > fromIndex ? targetIndex - 1 : targetIndex;
  const insertAt = zone === 'above' ? targetAfterRemoval : targetAfterRemoval + 1;
  if (insertAt === fromIndex) return rules as RewriteRule[];
  rest.splice(insertAt, 0, moved);
  return rest;
}

/**
 * 可以落库的那一份：剔除 `from` 与 `to` 都空白的占位行。
 *
 * 全空白行是用户刚点「添加规则」还没填的那一行，它的存在只在本地草稿里有意义。与后端
 * `core::url::clean_rewrites` 同一套判据。
 */
export function cleanPersistableRewrites(rules: readonly RewriteRule[]): RewriteRule[] {
  return rules
    .filter((rule) => rule.from.trim() !== '' || rule.to.trim() !== '')
    .map((rule) => ({
      from: rule.from,
      to: rule.to,
    }));
}

/**
 * 两份规则列表是否逐条相同。用于「草稿已经和落库的那份一样了吗」：一样就不发 IPC，
 * 也让外部更新能安全地接管草稿。
 */
export function areRewriteListsEqual(
  a: readonly RewriteRule[],
  b: readonly RewriteRule[],
): boolean {
  if (a.length !== b.length) return false;
  return a.every((rule, index) => rule.from === b[index]!.from && rule.to === b[index]!.to);
}
