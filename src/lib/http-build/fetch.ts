import { fromBuildInput, prepareRequest } from './prepare';
import type { BuildInput, BuiltFetch } from './types';

/**
 * 构造 `fetch(url, init)` 参数对。纯函数，无 I/O、无状态；`signal` 与 `redirect` 由调用方叠加。
 *
 * `prepareRequest` 之上的薄适配器。header 在这里收回 `Record`（`fetch` 的自然形状），代价是重名
 * header 只留最后一个值——该入口的既有局限，也是保留 tuple 形态的 `sendPreparedRequest` 走后台
 * 传输的原因。
 *
 * **body 只有文本形态**：`multipart` 与 `binary` 交给 `fetch` 的是一份描述（分片数组 / 文件
 * 引用），文件字节由服务端执行层按 `path` 现读。浏览器形态读不到本机路径，这条通道留给桌面
 * 与 `serve-web` 的后端执行，故 `fetch` 适配器只用于没有文件的请求。
 */
export function buildFetchInit(input: BuildInput): BuiltFetch {
  const prepared = prepareRequest(fromBuildInput(input));

  const headersRecord: Record<string, string> = {};
  for (const [k, v] of prepared.headers) headersRecord[k] = v;

  const init: Omit<RequestInit, 'headers'> & { headers: Record<string, string> } = {
    method: input.method,
    headers: headersRecord,
  };
  if (prepared.body !== null) init.body = prepared.body;

  return {
    url: prepared.url,
    init,
    bodyForLog: prepared.body ?? '',
  };
}
