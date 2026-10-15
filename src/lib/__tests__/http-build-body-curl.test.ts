/**
 * `http-build.ts` 的 body 包含规则、HTTP 方法保真与 cURL 一致性单测。
 *
 * 覆盖：`mode` 是否蕴含 body、空内容保护、全部 7 个动词的保真，以及
 * `buildCurlCommand` 与 `buildFetchInit` 的同源性。认证、模板与 `prepareRequest`
 * 核心在同目录的其它分片里。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildFetchInit, buildCurlCommand } from '../http-build';
import { kv, input } from './http-build-fixtures';
import type { AuthConfig, HttpMethod } from '../../types/domain';
// ============================================================
// E. body 是否包含
// ============================================================

describe('http-build: body inclusion', () => {
  it('mode=none → init.body is undefined', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'none', content: '' } }));
    assert.equal(init.body, undefined);
  });

  it('mode=none with content → still no body (mode wins)', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'none', content: 'should not be sent' } }),
    );
    assert.equal(init.body, undefined);
  });

  it('mode=json with content → body is the raw string', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'json', content: '{"a":1}' } }));
    assert.equal(init.body, '{"a":1}');
  });

  it('mode=form with content → body is the raw string', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'form', content: 'a=1&b=2' } }));
    assert.equal(init.body, 'a=1&b=2');
  });

  it('mode=raw with content → body is the raw string', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'raw', content: 'hello world' } }));
    assert.equal(init.body, 'hello world');
  });

  it('mode=json with empty content → no body (hasBody is false)', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'json', content: '' } }));
    assert.equal(init.body, undefined);
  });
});

// ============================================================
// F. HTTP 方法保真（全部 7 个动词）
// ============================================================

describe('http-build: HTTP method', () => {
  const METHODS: HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

  for (const m of METHODS) {
    it(`preserves ${m}`, () => {
      const { init } = buildFetchInit(input({ method: m }));
      assert.equal(init.method, m);
    });
  }
});

// ============================================================
// G. cURL 一致性
// ============================================================

describe('http-build: cURL parity with fetch', () => {
  it('produces equivalent URL (params + apikey-query)', () => {
    const i = input({
      url: 'http://api.example.com/x',
      params: [kv('page', '1')],
      auth: {
        kind: 'apikey',
        apikey: { key: 'k', value: 'v', inLocation: 'query' },
      } as AuthConfig,
    });
    const fetch = buildFetchInit(i);
    const curl = buildCurlCommand(i);
    assert.equal(fetch.url, 'http://api.example.com/x?page=1&k=v');
    assert.ok(curl.includes("'http://api.example.com/x?page=1&k=v'"));
  });

  it('emits the caller-joined URL when the input is a path', () => {
    // 回归网。`buildCurlCommand` 只对它拿到的 url 跑模板替换——它**不**join base URL。
    // 本文件其余测试都传完整 URL，所以渲染器真正产出的「只有 path」那种形状从
    // 来没被覆盖过，于是「复制为 cURL」发出的是 `curl -X POST '/cashier'`，根本跑
    // 不了。契约钉在这里：传进来什么 URL，就发出什么 URL。
    const i = input({ method: 'POST', url: 'https://api.example.com/cashier' });
    const curl = buildCurlCommand(i);
    assert.ok(
      curl.startsWith("curl -X POST 'https://api.example.com/cashier'"),
      `expected a runnable absolute URL, got: ${curl}`,
    );
  });

  it('includes Content-Type, Accept, and Authorization headers', () => {
    const i = input({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1}' },
      auth: { kind: 'bearer', bearer: 'eyJ.tok' } as AuthConfig,
    });
    const curl = buildCurlCommand(i);
    assert.ok(curl.includes("-H 'Content-Type: application/json'"));
    assert.ok(curl.includes("-H 'Accept: application/json'"));
    assert.ok(curl.includes("-H 'Authorization: Bearer eyJ.tok'"));
  });

  it('includes --data-raw when body present', () => {
    const i = input({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1}' },
    });
    const curl = buildCurlCommand(i);
    assert.ok(curl.includes(`--data-raw '{"a":1}'`));
  });

  it('omits --data-raw when body mode is none', () => {
    const i = input({ method: 'POST', body: { mode: 'none', content: '' } });
    const curl = buildCurlCommand(i);
    assert.ok(!curl.includes('--data-raw'));
  });

  it('escapes single quotes in shellQuote', () => {
    const i = input({
      method: 'POST',
      body: { mode: 'raw', content: "it's a 'test'" },
    });
    const curl = buildCurlCommand(i);
    // POSIX 单引号转义：' → '\''
    assert.ok(curl.includes(`--data-raw 'it'\\''s a '\\''test'\\'''`));
  });

  it('uses uppercase method verb', () => {
    const i = input({ method: 'POST', body: { mode: 'json', content: '{}' } });
    const curl = buildCurlCommand(i);
    assert.ok(curl.startsWith(`curl -X POST `));
  });

  it('contains every header from the fetch init (same set, possibly different order)', () => {
    const i = input({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1}' },
      headers: [kv('X-Trace', 'abc'), kv('X-Other', 'def')],
      auth: { kind: 'bearer', bearer: 'tok' } as AuthConfig,
    });
    const { init } = buildFetchInit(i);
    const curl = buildCurlCommand(i);
    const h = init.headers as Record<string, string>;
    for (const [k, v] of Object.entries(h)) {
      assert.ok(curl.includes(`-H '${k}: ${v}'`), `curl missing header ${k}: ${v}`);
    }
  });
});

// ============================================================
// H. 边界 / 跨界面的去重
// ============================================================

describe('http-build: edge cases', () => {
  it('composes params + apikey-query + body in a single URL', () => {
    const i = input({
      url: 'http://api.example.com/x',
      params: [kv('a', '1')],
      auth: {
        kind: 'apikey',
        apikey: { key: 'k', value: 'v', inLocation: 'query' },
      } as AuthConfig,
      body: { mode: 'json', content: '{"x":1}' },
    });
    const { url, init } = buildFetchInit(i);
    assert.equal(url, 'http://api.example.com/x?a=1&k=v');
    assert.equal(init.body, '{"x":1}');
  });

  it('treats a URL with an absolute scheme as-is (no re-prefixing)', () => {
    const { url } = buildFetchInit(
      input({ url: 'https://other.example.com/path', params: [kv('a', '1')] }),
    );
    assert.equal(url, 'https://other.example.com/path?a=1');
  });

  it('bodyForLog echoes the body content or empty string', () => {
    const a = buildFetchInit(input({ body: { mode: 'json', content: '{"x":1}' } }));
    const b = buildFetchInit(input({ body: { mode: 'none', content: '' } }));
    assert.equal(a.bodyForLog, '{"x":1}');
    assert.equal(b.bodyForLog, '');
  });
});
