import { prepareRequest } from '@/lib/http-build';
import type { PreparedRequest } from '@/lib/http-build';
import type { Environment, HttpMethod, RewriteRule, StoredRequestSnapshot } from '@/types/domain';
import type { EffectiveRequest } from './effectiveRequest';

/**
 * 组装这一次要发出的请求。发送前若动态变量解析出了新的值，调用方用新值再组装一次。
 *
 * `rewrites` 是环境里本模块那一格的有序路径前缀改写规则；它们只改写端点自己的 URL 路径。
 */
export function buildRequestInput(
  method: HttpMethod,
  urlPath: string,
  baseUrl: string,
  rewrites: readonly RewriteRule[],
  effective: EffectiveRequest,
  env: Environment | undefined,
  dynamicVars: Record<string, string>,
) {
  return prepareRequest({
    method,
    protocol: effective.protocol,
    graphql: effective.graphql,
    urlPath,
    baseUrl,
    rewrites,
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
 * 把一次发送出去的请求折成可落库的快照。收的是 `sendPreparedRequest` 返回的 `outbound`，
 * 不是预览那份：线上真正发出去的请求在 OAuth2 那一档多一个注入的 token。
 *
 * `wireHeaders` 是后端报上来的**线上那一份**请求头（含传输层补的 `accept` / `host` /
 * `content-length`）。缺席时回落到本地拼出来的那几条：请求从未抵达时后端报不上来。
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
    // 文件类请求体（`binary` / 带文件分片的 `multipart`）存的就是那份描述：文件只记路径，
    // 字节在发请求时按路径现读，故历史里留得下、也读得回去的都是文本。
    body: request.body,
  };
}
