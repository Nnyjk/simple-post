/**
 * 响应历史的保留条数 —— 共享的常量与纯函数。
 *
 * 住在自己的文件而非某一个切片里：`ui-prefs` 持有这个数，`ui-response` 按它裁历史
 * 列表，两个切片互相不 import（它们的约定就是只经 `AppState` 共享状态），故共用的
 * 规则与常量放这里。
 */
import type { ResponseHistoryEntry } from '@/types/domain';

/**
 * 每个接口保留多少条**未收藏**发送历史的缺省值。
 *
 * 与后端 `domain::RESPONSE_HISTORY_KEEP_DEFAULT` 相同：两侧的缺省值必须是同一个数，
 * 否则「没配置过时」界面按一个数裁、库里按另一个数裁。
 */
export const RESPONSE_HISTORY_KEEP_DEFAULT = 10;

/**
 * 保留条数的上限。
 *
 * 与后端 `preferences::RESPONSE_HISTORY_KEEP_MAX` 相同。再大的值也没有意义：历史条
 * 一次显示不完那么多行，而每一行都在用户的库里占着地方。
 */
export const RESPONSE_HISTORY_KEEP_MAX = 500;

/** 把保留条数夹到 `[1, RESPONSE_HISTORY_KEEP_MAX]`。非数字回到缺省值。 */
export function clampHistoryKeep(keep: number): number {
  if (!Number.isFinite(keep)) return RESPONSE_HISTORY_KEEP_DEFAULT;
  return Math.min(Math.max(Math.round(keep), 1), RESPONSE_HISTORY_KEEP_MAX);
}

/**
 * 丢掉超出条数的**未收藏**行，最旧的先丢。收藏的行一律不动。
 *
 * 输入是「最新在末尾」的次序，输出保持同一次序，故被丢掉的总是最旧的那几行。
 */
export function capUnfavorited(list: ResponseHistoryEntry[], keep: number): ResponseHistoryEntry[] {
  const unfavorited = list.filter((e) => !e.isFavorite);
  if (unfavorited.length <= keep) return list;
  const dropped = new Set(unfavorited.slice(0, unfavorited.length - keep).map((e) => e.id));
  return list.filter((e) => !dropped.has(e.id));
}
