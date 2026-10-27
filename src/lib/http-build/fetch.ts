import { fromBuildInput, prepareRequest } from './prepare';
import type { BuildInput, BuiltFetch } from './types';

/**
 * 构造 `fetch(url, init)` 参数对。纯函数，无 I/O、无状态；`signal` 与 `redirect`
 * 由调用方叠加。
 *
 * `prepareRequest` 之上的薄适配器。header 在这里收回 `Record`（`fetch` 的自然形状），
 * 代价是重名 header 只留最后一个值——该入口的既有局限，也是保留 tuple 形态的
 * `sendPreparedRequest` 走后台传输的原因。
 */
export function buildFetchInit(input: BuildInput): BuiltFetch {
  const prepared = prepareRequest(fromBuildInput(input));

  const headersRecord: Record<string, string> = {};
  for (const [k, v] of prepared.headers) headersRecord[k] = v;

  const init: RequestInit = {
    method: input.method,
    headers: headersRecord,
  };
  if (prepared.body !== null) {
    init.body = prepared.body;
  }

  return {
    url: prepared.url,
    init,
    bodyForLog: prepared.body ?? '',
  };
}
