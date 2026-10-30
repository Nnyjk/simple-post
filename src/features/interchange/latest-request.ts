/**
 * 只认最新的一趟请求。
 *
 * 发请求时领一个号，回来时比一下：号不是最新的那趟就是**过期消息**，丢弃。
 * 连着换两个环境时先发的那一趟可能后到，它带回来的地址属于旧环境——
 * 写进去的话，提示又会按旧环境点名。
 */
export interface LatestRequest {
  /** 领一个号。 */
  next(): number;
  /** 这个号还是最新的吗。 */
  isCurrent(ticket: number): boolean;
  /** 作废在途的请求（弹窗关掉、重新打开）。 */
  invalidate(): void;
}

export function createLatestRequest(): LatestRequest {
  let current = 0;
  return {
    next: () => ++current,
    isCurrent: (ticket: number) => ticket === current,
    invalidate: () => {
      current += 1;
    },
  };
}
