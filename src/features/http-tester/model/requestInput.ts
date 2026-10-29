import { prepareRequest } from '@/lib/http-build';
import type { PreparedRequest } from '@/lib/http-build';
import type { Environment, HttpMethod, StoredRequestSnapshot } from '@/types/domain';
import type { EffectiveRequest } from './effectiveRequest';

/**
 * 组装这一次要发出的请求。
 *
 * 发送前若动态变量解析出了新的值，调用方用新值再组装一次。
 */
export function buildRequestInput(
  method: HttpMethod,
  urlPath: string,
  baseUrl: string,
  effective: EffectiveRequest,
  env: Environment | undefined,
  dynamicVars: Record<string, string>,
) {
  return prepareRequest({
    method,
    urlPath,
    baseUrl,
    params: effective.params,
    pathParams: effective.pathParams,
    headers: effective.headers,
    body: effective.body,
    auth: effective.auth,
    env,
    dynamicVars,
  });
}

/**
 * 把一次发送出去的请求折成可落库的快照。
 *
 * 收的是 `sendPreparedRequest` 返回的 `outbound`，不是预览那份：线上真正发出去的
 * 请求在 OAuth2 那一档多一个注入的 token，存预览那份会让历史行少显示那个凭据。
 *
 * `wireHeaders` 是后端报上来的**线上那一份**请求头（含传输层补的 `accept` /
 * `accept-encoding` / `host` / `content-length`）。缺席时回落到本地拼出来的那几条：
 * 请求从未抵达时（超时、网络错误）后端报不上来，但界面上拼的那份仍然成立。
 */
export function toRequestSnapshot(
  request: PreparedRequest,
  wireHeaders?: ReadonlyArray<readonly [string, string]>,
): StoredRequestSnapshot {
  const headers = wireHeaders ?? request.headers;
  return {
    method: request.method,
    url: request.url,
    headers: headers.map(([name, value]) => [name, value] as [string, string]),
    body: request.body,
    // 二进制请求体的字节（`binary` 模式，或带文件分片的 multipart）走这一条通道：
    // 没有它，历史行里的请求侧栏会显示成一个没有 body 的请求，而那次调用明明带着文件。
    bodyBase64: request.bodyBase64,
  };
}
