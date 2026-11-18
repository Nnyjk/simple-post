/**
 * 汇总报告 —— 由逐条结果算出来，不另记一份。报告的恒等式：**总数 = 状态码分布之和 +
 * 没拿到状态码 + 压根没发出去 + 未执行**。四类互斥且穷尽，故报告不会算出一个对不上的总数。
 */
import type { BatchOutcome, BatchReport } from './types';

/**
 * 算一份报告。`total` 是这一批**共有**多少条（不是跑完了多少条）：中止之后没轮到的那几条
 * 也在这个数里，它们由 `notRun` 单独报出。协议不在批量范围内的条目（SSE、WebSocket）同样
 * 计入 `notRun`：执行器跳过了它们，既不算成功也不算失败。
 *
 * `outcomes` 的条数**不得超过** `total`：每条结论对应这一批里的一条接口，多出来的结论说明
 * 调用方把两个批次的结果混在了一起。宁可当场报错，也不要用 `Math.max(0, …)` 把它压成 0
 * 藏起来 —— 恒等式正是靠这个数守住的。
 */
export function buildReport(
  total: number,
  outcomes: readonly BatchOutcome[],
  elapsedMs: number,
): BatchReport {
  if (outcomes.length > total) {
    throw new Error(`批量报告的结论条数（${outcomes.length}）超过了这一批的总数（${total}）`);
  }
  let success = 0;
  let failed = 0;
  let blocked = 0;
  let noResponse = 0;
  let skipped = 0;
  let totalDurationMs = 0;
  const byStatus = new Map<number, number>();

  for (const outcome of outcomes) {
    totalDurationMs += outcome.durationMs;
    // 主动跳过的协议既不是成功也不是失败：它没发出去，也不该让用户以为这一批里有错误。
    // 它们与「中止之后没轮到」同属未执行那一档。
    if (outcome.failure === 'skipped') {
      skipped++;
      continue;
    }
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
    notRun: total - outcomes.length + skipped,
    statusCounts,
    noResponse,
    blocked,
    totalDurationMs,
    elapsedMs,
  };
}
