/**
 * 发送前的路径参数闸门。
 *
 * 路径参数按 OpenAPI 惯例是必填的：带着 `{id}` 发出去只会换一个含义不明的 404，用户
 * 看不出是自己漏填了一个值。单次 HTTP 发送、SSE 建连、WebSocket 建连三处都过这一道，
 * 各自判一遍必然走样，故判断只写在这里。
 */
import type { PreparedRequest } from '@/lib/http-build/types';

/** 这次请求还有路径参数没取到值。 */
export function pathParamsBlocked(request: PreparedRequest): boolean {
  return request.missingPathParams.length > 0;
}

/**
 * 闸门开着就返回 `true`；关着时先把缺值名单交给界面（切到路径参数页），再返回 `false`。
 *
 * 名单里可能混着模块地址的占位符 —— 那一份不在本页可填，提示文案由
 * `buildPathParamError` 负责说清。
 */
export function pathParamsReady(
  request: PreparedRequest,
  onMissing: (missing: string[]) => void,
): boolean {
  onMissing(request.missingPathParams);
  return !pathParamsBlocked(request);
}
