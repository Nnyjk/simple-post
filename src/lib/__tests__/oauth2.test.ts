/**
 * `oauth2.ts`（token 注入 + 取 token 的唯一入口）的纯单测。通过 `npm test` 运行
 * （它调用 `tsx --test`）。
 *
 * 注入规则本身（query / header 两种位置、编码、参数名为空时的回退、查重）由共享样本
 * `fixtures/oauth2-parity.json` 钉住，Rust 那一侧读同一份文件（读取器在
 * `oauth2-parity.test.ts`）。本文件只覆盖样本没覆盖的那部分，以及两个纯函数：
 *
 *   A. query 档不动 header
 *   B. header 档不动 URL
 *   C. 纯度 —— `applyOAuth2Token` 不改传入对象
 *   D. 非 oauth2 的 auth 原样返回
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  applyOAuth2Token,
  buildAuthHeaderValue,
  buildTokenQuery,
  hasAuthorizationHeader,
} from '../oauth2';
import { prepareRequest, type PrepareRequestInput, type PreparedRequest } from '../http-build';
import type { AuthConfig, KeyValue, OAuth2Auth, OAuth2TokenResult } from '../../types/domain';

/** `prepareRequest` 的 header 入参是 `KeyValue[]`（不是元组列表——元组是它的**产出**）。 */
function kv(key: string, value: string, enabled = true): KeyValue {
  return { id: `k_${key}`, key, value, enabled };
}

// ---------- 共用夹具 ----------

function oauth2(overrides: Partial<OAuth2Auth> = {}): AuthConfig {
  return {
    kind: 'oauth2',
    oauth2: {
      grantType: 'client_credentials',
      tokenUrl: 'https://idp.example.com/token',
      clientId: 'cid',
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
      ...overrides,
    },
  };
}

function token(overrides: Partial<OAuth2TokenResult> = {}): OAuth2TokenResult {
  return {
    accessToken: 'eyJ.tok',
    tokenType: 'Bearer',
    expiresAt: 1800000000,
    scope: null,
    refreshed: true,
    ...overrides,
  };
}

function prepared(overrides: Partial<PrepareRequestInput> = {}): PreparedRequest {
  return prepareRequest({
    method: 'GET',
    urlPath: '/x',
    baseUrl: 'https://api.example.com',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    ...overrides,
  });
}

/** 元组列表 → 查找表，供「某个 header 存不存在」这类断言用。 */
function headerMap(tuples: ReadonlyArray<readonly [string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of tuples) out[k] = v;
  return out;
}

// ============================================================
// A. query 位置
// ============================================================

describe('oauth2: query 位置的 token', () => {
  it('query 档不动 header', () => {
    const req = prepared({ headers: [kv('X-Trace', 'abc')] });
    const out = applyOAuth2Token(req, oauth2({ tokenLocation: 'query' }), token());
    assert.deepEqual(headerMap(out.headers), { 'X-Trace': 'abc', Accept: 'application/json' });
  });
});

describe('oauth2: buildTokenQuery', () => {
  it('就是一次拼串，与 auth 配置无关', () => {
    assert.equal(
      buildTokenQuery('https://h/p', 'access_token', token({ accessToken: 't' })),
      'https://h/p?access_token=t',
    );
    assert.equal(
      buildTokenQuery('https://h/p?a=1', '', token({ accessToken: 't' })),
      'https://h/p?a=1&access_token=t',
    );
  });
});

// ============================================================
// B. header 位置
// ============================================================

describe('oauth2: header 位置的 token', () => {
  it('header 档不动 URL', () => {
    const req = prepared({ urlPath: 'x?a=1' });
    const out = applyOAuth2Token(req, oauth2(), token());
    assert.equal(out.url, 'https://api.example.com/x?a=1');
  });
});

describe('oauth2: hasAuthorizationHeader / buildAuthHeaderValue', () => {
  it('按小写比较找 Authorization', () => {
    assert.equal(hasAuthorizationHeader([['AUTHORIZATION', 'x']]), true);
    assert.equal(hasAuthorizationHeader([['Accept', 'application/json']]), false);
    assert.equal(hasAuthorizationHeader([]), false);
  });

  it('拼 <tokenType> <token>，类型空白则只给裸 token', () => {
    assert.equal(buildAuthHeaderValue(token({ tokenType: 'Bearer' })), 'Bearer eyJ.tok');
    assert.equal(buildAuthHeaderValue(token({ tokenType: '' })), 'eyJ.tok');
  });
});

// ============================================================
// C. 纯度 —— `prepared` 被 cURL 预览共享
// ============================================================

describe('oauth2: applyOAuth2Token 不改传入的 request', () => {
  it('header 档：原对象的 headers 长度与内容不变', () => {
    const req = prepared({ headers: [kv('X-Trace', 'abc')] });
    const before = req.headers.length;
    const out = applyOAuth2Token(req, oauth2(), token());
    assert.equal(req.headers.length, before);
    assert.equal(headerMap(req.headers)['Authorization'], undefined);
    // 结果是新对象，且它自己多了那条
    assert.notEqual(out, req);
    assert.equal(out.headers.length, before + 1);
    assert.equal(headerMap(out.headers)['Authorization'], 'Bearer eyJ.tok');
  });

  it('query 档：原对象的 url 不变', () => {
    const req = prepared();
    const before = req.url;
    const out = applyOAuth2Token(req, oauth2({ tokenLocation: 'query' }), token());
    assert.equal(req.url, before);
    assert.notEqual(out.url, before);
    assert.equal(out.url, 'https://api.example.com/x?access_token=eyJ.tok');
  });

  it('已有 Authorization 时返回的仍是同一个对象（无注入可做）', () => {
    const req = prepared({ headers: [kv('Authorization', 'Bearer mine')] });
    assert.equal(applyOAuth2Token(req, oauth2(), token()), req);
  });
});

// ============================================================
// D. 非 oauth2 的 auth
// ============================================================

describe('oauth2: 非 oauth2 的 auth 原样返回', () => {
  it('kind 为 bearer 时不注入', () => {
    const req = prepared();
    assert.equal(applyOAuth2Token(req, { kind: 'bearer', bearer: 'x' }, token()), req);
  });

  it('kind 是 oauth2 但载荷缺省时按「没配 oauth2」处理', () => {
    const req = prepared();
    assert.equal(applyOAuth2Token(req, { kind: 'oauth2' }, token()), req);
  });
});
