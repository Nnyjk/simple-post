/**
 * 流式建连前的鉴权收尾。
 *
 * SSE 与 WS 直接拿 `prepared.headers` / `prepared.url` 去建连，而 OAuth2 的 token 不在里头
 * （它要发网络请求才拿得到），故由 `applyStreamAuth` 在建连前取好并注入。这里钉住三条：
 *
 *   1. 非 OAuth2 一条路都不走 —— 那几种鉴权已在 `prepareRequest` 里折算过；
 *   2. OAuth2 成功时把取到的 token 交给注入函数，返回注入后的请求；
 *   3. OAuth2 失败时只回错误文案，没有 `request` 键（调用方据此不建连）。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { applyStreamAuth } from '../stream-auth';
import type { PreparedRequest } from '@/lib/http-build';
import type { AuthConfig, OAuth2TokenResult } from '@/types/domain';

const REQUEST: PreparedRequest = {
  method: 'GET',
  url: 'wss://example.test/stream',
  headers: [['Accept', '*/*']],
  body: null,
  bodyMode: 'none',
  missingVars: [],
  vars: { tenant: 'acme' },
  missingPathParams: [],
  problems: [],
};

const OAUTH2: AuthConfig = {
  kind: 'oauth2',
  oauth2: {
    grantType: 'client_credentials',
    tokenUrl: 'https://idp.test/token',
    clientId: 'id',
    clientSecret: 'secret',
    clientAuth: 'basic',
    scope: '',
    authorizationUrl: '',
    redirectUri: '',
    codeVerifier: '',
    authCode: '',
    username: '',
    password: '',
    extraParams: [],
    tokenLocation: 'header',
    tokenParam: '',
  },
};

const TOKEN: OAuth2TokenResult = {
  accessToken: 'tok-1',
  tokenType: 'Bearer',
  expiresAt: 1,
  scope: '',
  refreshed: false,
};

function deps(overrides: {
  fetchOAuth2Token?: () => Promise<OAuth2TokenResult>;
  applyOAuth2Token?: (request: PreparedRequest) => PreparedRequest;
}) {
  const calls = { fetch: 0, apply: 0, token: undefined as OAuth2TokenResult | undefined };
  return {
    calls,
    value: {
      fetchOAuth2Token: async () => {
        calls.fetch++;
        return overrides.fetchOAuth2Token ? overrides.fetchOAuth2Token() : TOKEN;
      },
      applyOAuth2Token: (request: PreparedRequest) => {
        calls.apply++;
        calls.token = TOKEN;
        return overrides.applyOAuth2Token ? overrides.applyOAuth2Token(request) : request;
      },
    },
  };
}

describe('applyStreamAuth', () => {
  it('非 OAuth2 时原样返回请求，两个依赖都不调用', async () => {
    const fake = deps({});
    const result = await applyStreamAuth(
      { request: REQUEST, auth: { kind: 'bearer', bearer: 'abc' } },
      fake.value,
    );
    if ('error' in result) assert.fail(result.error);
    assert.equal(result.request, REQUEST);
    assert.equal(fake.calls.fetch, 0);
    assert.equal(fake.calls.apply, 0);
  });

  it('OAuth2 成功时把 token 交给注入函数，返回它的输出', async () => {
    const injected: PreparedRequest = {
      ...REQUEST,
      headers: [...REQUEST.headers, ['Authorization', 'Bearer tok-1']],
    };
    const fake = deps({ applyOAuth2Token: () => injected });
    const result = await applyStreamAuth({ request: REQUEST, auth: OAUTH2 }, fake.value);
    assert.equal(fake.calls.fetch, 1);
    assert.equal(fake.calls.apply, 1);
    assert.equal(fake.calls.token, TOKEN);
    if ('error' in result) assert.fail(result.error);
    assert.equal(result.request, injected);
  });

  it('OAuth2 失败时只回错误文案，没有 request 键', async () => {
    const fake = deps({
      fetchOAuth2Token: () => Promise.reject(new Error('token 端点回了 401')),
    });
    const result = await applyStreamAuth({ request: REQUEST, auth: OAUTH2 }, fake.value);
    assert.deepEqual(result, { error: 'token 端点回了 401' });
    assert.equal(fake.calls.apply, 0);
  });
});
