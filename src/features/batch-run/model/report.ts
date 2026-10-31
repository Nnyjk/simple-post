/**
 * 汇总报告 —— 由逐条结果算出来，不另记一份。
 *
 * 报告的恒等式：**总数 = 状态码分布之和 + 没拿到状态码 + 压根没发出去 + 未执行**。
 * 四类互斥且穷尽，故报告不会算出一个对不上的总数。
 */
import type { BatchOutcome, BatchReport } from './types';

/**
 * 算一份报告。
 *
 * `total` 是这一批**共有**多少条（不是跑完了多少条）：中止之后没轮到的那几条也在这
 * 个数里，它们由 `notRun` 单独报出。
 */
export function buildReport(
  total: number,
  outcomes: readonly BatchOutcome[],
  elapsedMs: number,
): BatchReport {
  let success = 0;
  let failed = 0;
  let blocked = 0;
  let noResponse = 0;
  let totalDurationMs = 0;
  const byStatus = new Map<number, number>();

  for (const outcome of outcomes) {
    totalDurationMs += outcome.durationMs;
    if (outcome.success) {
      success++;
    } else {
      failed++;
    }
    // 「压根没发出去」与「发了但没拿到响应」是两回事：前者是这条没准备好，后者是
    // 发出去了但对方没应答。两者都没有状态码，故分开报。
    if (outcome.failure === 'blocked') {
      blocked++;
      continue;
    }
    if (outcome.status > 0) {
      byStatus.set(outcome.status, (byStatus.get(outcome.status) ?? 0) + 1);
    } else {
      noResponse++;
    }
  }

  const statusCounts = [...byStatus.entries()]
    .map(([status, count]) => ({ status, count }))
    .sort((a, b) => a.status - b.status);

  return {
    total,
    success,
    failed,
    notRun: Math.max(0, total - outcomes.length),
    statusCounts,
    noResponse,
    blocked,
    totalDurationMs,
    elapsedMs,
  };
}
