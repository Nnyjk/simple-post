import { fromBuildInput, prepareRequest } from './prepare';
import { base64ToBytes } from '../base64';
import type { BuildInput, BuiltFetch } from './types';

/**
 * 构造 `fetch(url, init)` 参数对。纯函数，无 I/O、无状态；`signal` 与 `redirect`
 * 由调用方叠加。
 *
 * `prepareRequest` 之上的薄适配器。header 在这里收回 `Record`（`fetch` 的自然形状），
 * 代价是重名 header 只留最后一个值——该入口的既有局限，也是保留 tuple 形态的
 * `sendPreparedRequest` 走后台传输的原因。
 *
 * **body 有两种形态**：文本直接给字符串；二进制（`binary` 模式，或带文件分片的
 * multipart）解成字节给 `fetch`。字节形态不能退成字符串——那会把文件按 UTF-8 解一遍，
 * 字节已经不是原来那些了。
 */
export function buildFetchInit(input: BuildInput): BuiltFetch {
  const prepared = prepareRequest(fromBuildInput(input));

  const headersRecord: Record<string, string> = {};
  for (const [k, v] of prepared.headers) headersRecord[k] = v;

  const init: RequestInit = {
    method: input.method,
    headers: headersRecord,
  };
  if (prepared.bodyBase64 !== null) {
    init.body = base64ToBytes(prepared.bodyBase64);
  } else if (prepared.body !== null) {
    init.body = prepared.body;
  }

  return {
    url: prepared.url,
    init,
    // 二进制 body 没有字符串形态，回显写成一行说明，读者知道它不是空的。
    bodyForLog: prepared.body ?? (prepared.bodyBase64 === null ? '' : '[二进制请求体]'),
    bodyBase64: prepared.bodyBase64,
  };
}
