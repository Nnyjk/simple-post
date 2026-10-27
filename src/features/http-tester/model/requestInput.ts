import { prepareRequest } from '@/lib/http-build';
import type { Environment, HttpMethod } from '@/types/domain';
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
