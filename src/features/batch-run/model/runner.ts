/**
 * 批量运行的执行器 —— 顺序发，逐条报，可中止。
 *
 * 刻意**不做并发**：环境变量的写回由脚本产生，并发时写回的先后不可预期，报告也就
 * 不可复现。顺序执行时「第几条看到的是哪一份变量」是确定的。
 *
 * 执行器不认识 HTTP：它只按顺序调用调用方给的 `send`，并把每一条的结论收齐。因此
 * 它可以在没有后端的情况下测。
 */
import { buildReport } from './report';
import type { BatchItem, BatchOutcome, BatchReport } from './types';

/** 一次批量运行的结果。 */
export interface BatchRunResult {
  /** 跑完的那些条的结论，按执行顺序。 */
  outcomes: BatchOutcome[];
  /** 汇总报告。 */
  report: BatchReport;
  /** 用户中止过整批。 */
  aborted: boolean;
}

/** 执行器的可选回调。 */
export interface BatchRunOptions {
  /** 中止信号。中止后不再开始新的请求；在途的那一条由 `send` 自己处理。 */
  signal: AbortSignal;
  /** 每跑完一条调一次，参数是这条的结论与它在批次里的序号。 */
  onOutcome?: (outcome: BatchOutcome, index: number) => void;
  /** 开始跑某一条之前调一次，参数是它在批次里的序号。 */
  onStart?: (index: number) => void;
}

/**
 * 顺序跑一批端点。
 *
 * 单条失败**不中断**整批：`send` 抛异常或返回失败的结论，都只是这一条的结论，下
 * 一条照发。中止时不再开始新的请求，已经跑完的那些保留在结果里。
 */
export async function runBatch(
  items: readonly BatchItem[],
  send: (item: BatchItem, signal: AbortSignal) => Promise<BatchOutcome>,
  options: BatchRunOptions,
): Promise<BatchRunResult> {
  const outcomes: BatchOutcome[] = [];
  const startedAt = Date.now();
  let aborted = false;

  for (let index = 0; index < items.length; index++) {
    if (options.signal.aborted) {
      aborted = true;
      break;
    }
    const item = items[index];
    options.onStart?.(index);
    let outcome: BatchOutcome;
    try {
      outcome = await send(item, options.signal);
    } catch (err) {
      // `sendPreparedRequest` 有文档保证不抛，但组装请求这一步（读环境、解 JSON）
      // 不在那个保证里。一条抛了不能让整批停下来，故就地记成一条失败。
      outcome = {
        endpointId: item.endpointId,
        name: item.name,
        method: item.method,
        success: false,
        status: 0,
        statusText: 'Error',
        durationMs: 0,
        failure: 'transport',
        error: err instanceof Error ? err.message : String(err),
      };
    }
    outcomes.push(outcome);
    options.onOutcome?.(outcome, index);
  }

  if (options.signal.aborted) aborted = true;
  const report = buildReport(items.length, outcomes, Date.now() - startedAt);
  return { outcomes, report, aborted };
}
