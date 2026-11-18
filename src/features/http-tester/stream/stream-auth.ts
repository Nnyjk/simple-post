/**
 * 流式建连之前的鉴权收尾：OAuth2 的 token 要发一次网络请求才拿得到，故在打开传输之前取好并
 * 注入。规则与 `src/lib/http.ts::sendPreparedRequest` 同一条：同一对 `fetchOAuth2Token` /
 * `applyOAuth2Token`，同一个变量池（`request.vars`），同一个错误来源（异常自己的 message）。
 *
 * Bearer / Basic / API key 已经在 `prepareRequest` 里折进 `prepared.headers`（`inLocation` 为
 * query 的 API key 折进 URL），这里不再动它们：再注入一次会在线上多出一条重复 header。
 */
import type { AuthConfig, OAuth2TokenResult } from '@/types/domain';
import type { PreparedRequest } from '@/lib/http-build';
import { applyOAuth2Token, fetchOAuth2Token } from '@/lib/oauth2';

/** 取 token 与注入两份纯 I/O / 纯函数；注入只为测试能换掉它们。 */
export interface StreamAuthDeps {
  fetchOAuth2Token: (
    auth: AuthConfig,
    options?: { vars?: Record<string, string> },
  ) => Promise<OAuth2TokenResult>;
  applyOAuth2Token: (
    request: PreparedRequest,
    auth: AuthConfig,
    token: OAuth2TokenResult,
  ) => PreparedRequest;
}

export type StreamAuthResult = { request: PreparedRequest } | { error: string };

export async function applyStreamAuth(
  input: { request: PreparedRequest; auth: AuthConfig | undefined },
  deps: StreamAuthDeps = { fetchOAuth2Token, applyOAuth2Token },
): Promise<StreamAuthResult> {
  const auth = input.auth;
  // 只有 OAuth2 需要在这里动手：它的 token 只能取，剩下的几种在组装时就已经落进请求里了。
  if (auth?.kind !== 'oauth2' || !auth.oauth2) return { request: input.request };
  try {
    const token = await deps.fetchOAuth2Token(auth, { vars: input.request.vars });
    return { request: deps.applyOAuth2Token(input.request, auth, token) };
  } catch (err) {
    // 与发送链路同一个来源：`sendPreparedRequest` 也是取 `(err as Error)?.message`。
    return { error: (err as Error)?.message ?? String(err) };
  }
}
