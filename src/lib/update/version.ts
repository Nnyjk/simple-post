/**
 * 版本号比较 —— 「有没有新版」的最终口子。
 *
 * 规则与 `crates/simple-post-core/src/update/version.rs` 一致：去前导 `v`、切掉第一个 `-`
 * 之后的预发布段、逐段按数字比、缺段按 0。读不懂的版本号不参与比较（见
 * [`shouldPromptUpdate`]）：把格式错误当成「有新版本」会推用户去装一个不存在的东西。
 */

/** 解析成数字段。空串、空段、非数字一律 `null`。 */
function parse(raw: string | null | undefined): number[] | null {
  if (typeof raw !== 'string') return null;
  const core = raw.trim().replace(/^v/i, '').split('-')[0] ?? '';
  if (core === '') return null;
  const numbers: number[] = [];
  for (const part of core.split('.')) {
    if (!/^\d+$/.test(part)) return null;
    numbers.push(Number(part));
  }
  return numbers;
}

/**
 * 比两个版本号：`a > b` 为 1，相等 0，`a < b` 为 -1；任一方解析失败为 `null`。
 *
 * `1.2` 与 `1.2.0` 相等——发布标签偶尔少写一段，不该因此被判成「有新版本」。
 */
export function compareVersions(
  a: string | null | undefined,
  b: string | null | undefined,
): number | null {
  const left = parse(a);
  const right = parse(b);
  if (!left || !right) return null;
  const length = Math.max(left.length, right.length);
  for (let i = 0; i < length; i += 1) {
    const l = left[i] ?? 0;
    const r = right[i] ?? 0;
    if (l !== r) return l > r ? 1 : -1;
  }
  return 0;
}

/**
 * 「这一次要不要提示有新版本」。`reported` 是后端 / 插件自己的结论，这里只用固定的当前
 * 版本复核一件事：`latest ≤ current` 一律不提示。
 *
 * 当前版本为空、或某一侧版本号读不懂时退回 `reported`——没有更可靠的依据，宁可提示，
 * 也不吞掉一次真更新。
 */
export function shouldPromptUpdate(
  reported: boolean,
  latest: string | null | undefined,
  current: string | null | undefined,
): boolean {
  if (!reported) return false;
  if (!current) return true;
  const order = compareVersions(latest, current);
  return order === null ? true : order > 0;
}
