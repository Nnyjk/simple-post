/**
 * 集合子树的端点清单与顺序 —— 状态机、逐条结果与汇总报告。
 *
 * 逐条结果（[`BatchOutcome`]）与汇总报告（[`BatchReport`]）是**两份**东西：前者回答
 * 「这一条怎么样了」，后者回答「这一批怎么样」。报告的每一项都能由逐条结果算出来，
 * 故报告不是另一份记录，而是同一批事实的另一种读法。
 */
import type { EnvWrite } from '@/types/domain';

/**
 * 一个端点在批量运行中的状态。
 *
 * `pending` → `running` → `success` | `failed`。运行结束时仍是 `pending` 的那些是
 * **未执行**——中止之后没轮到它们。
 */
export type BatchItemState = 'pending' | 'running' | 'success' | 'failed';

/**
 * 一条失败的分类。
 *
 *   - `http`       —— 拿到了响应，但状态码不是 2xx；
 *   - `transport`  —— 压根没拿到响应（超时、网络错误）；
 *   - `blocked`    —— 压根没发出去（缺路径参数、body 解析不了）；
 *   - `aborted`    —— 用户中止了在途的那一条。
 */
export type BatchFailureKind = 'http' | 'transport' | 'blocked' | 'aborted';

/** 批量运行里的一条：一个待发送的端点。 */
export interface BatchItem {
  /** 端点 id。 */
  endpointId: string;
  /** 端点名，报告里显示。 */
  name: string;
  /** 请求方法。 */
  method: string;
  /** 端点自己存的 URL（path 部分）。 */
  url: string;
}

/** 一条端点跑完之后的结论。 */
export interface BatchOutcome {
  endpointId: string;
  name: string;
  method: string;
  /** 成功与否。 */
  success: boolean;
  /** HTTP 状态码。没拿到响应时是 0。 */
  status: number;
  /** 状态文本。 */
  statusText: string;
  /** 这一条花了多久。 */
  durationMs: number;
  /** 失败分类。成功时是 `null`。 */
  failure: BatchFailureKind | null;
  /** 给人看的失败原因。成功时缺席。 */
  error?: string;
  /** 没解析出来的变量名。单次发送不拦它们，故这里只是提示。 */
  missingVars?: string[];
  /** 没取到值的路径占位符名。取到它时这一条压根没发出去。 */
  missingPathParams?: string[];
  /**
   * 脚本写回的环境变量。
   *
   * 落库由后端做（那是唯一那条持久化路径），本地副本等**整批跑完**再统一同步：中途
   * 同步会让后面那几条看到被改过的变量，快照隔离就破了。
   */
  envWrites?: EnvWrite[];
  /** 上面那些写入该落到哪个环境。 */
  envId?: string;
}

/** 一批跑完之后的汇总。 */
export interface BatchReport {
  /** 这一批共有多少条。 */
  total: number;
  /** 成功（2xx）的条数。 */
  success: number;
  /** 失败的条数：非 2xx、传输失败、被拦下、中止，都算。 */
  failed: number;
  /** 中止之后压根没轮到执行的条数。 */
  notRun: number;
  /** 状态码分布，只统计真正拿到状态码的那些。按状态码升序。 */
  statusCounts: Array<{ status: number; count: number }>;
  /** 没拿到状态码的条数：超时、网络错误、中止。 */
  noResponse: number;
  /** 压根没发出去的条数：缺路径参数、body 解析不了。 */
  blocked: number;
  /** 各条耗时之和。 */
  totalDurationMs: number;
  /** 整批的墙钟耗时。 */
  elapsedMs: number;
}
