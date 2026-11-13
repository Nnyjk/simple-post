/**
 * 更新检查的时间规则。两件事都是纯函数：输入时间戳与「现在」，输出布尔值或文案，
 * 故节流与相对时间能单独测，不必启动 store 或等真实时间流过。
 */

/** 启动自动检查的节流窗口。24 小时。 */
export const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/**
 * 该不该发起一次检查。
 *
 * - 从未检查过（`null` / 空串 / 解析不出来）→ 该查。
 * - 记录的时间落在未来（用户改过系统时钟、时区乱跳）→ 该查：一个未来的时间戳若按
 *   「还没到点」处理，应用会一直不查，而用户什么也看不出来。
 * - 距离上次检查已满 `intervalMs` → 该查。
 */
export function dueForCheck(
  lastCheckedAt: string | null | undefined,
  now: number,
  intervalMs: number = UPDATE_CHECK_INTERVAL_MS,
): boolean {
  if (!lastCheckedAt) return true;
  const last = Date.parse(lastCheckedAt);
  if (Number.isNaN(last)) return true;
  if (last > now) return true;
  return now - last >= intervalMs;
}

/** 一分钟以内不显示「0 分钟前」这类噪音，统一说「刚刚」。 */
const JUST_NOW_MS = 60_000;

/**
 * 把一个 ISO 时间写成中文相对时间：「刚刚」「3 小时前」「2 天前」。
 *
 * `iso` 为 `null` / 空串 / 解析不出来时返回 `null`，由调用方决定说什么（设置卡片说的是
 * 「尚未检查」）——这里不替调用方编一句文案。
 */
export function relativeTime(iso: string | null | undefined, now: number): string | null {
  if (!iso) return null;
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return null;
  // 未来时间戳（时钟回拨）说「刚刚」而不是「-3 小时前」。
  const diff = Math.max(0, now - then);
  if (diff < JUST_NOW_MS) return '刚刚';
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} 天前`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months} 个月前`;
  return `${Math.floor(months / 12)} 年前`;
}

/**
 * 把一个 ISO 时间写成日期：「2026-11-13」。更新对话框里显示发布时间用它：
 * 「3 小时前」适合回答「什么时候查的」，而发布日是 `latest.json` 里的一个日期，
 * 写成相对时间反而读不出那个版本有多老。
 */
export function formatDate(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return null;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
