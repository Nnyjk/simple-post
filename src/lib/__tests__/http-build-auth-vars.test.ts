/**
 * `http-build.ts` 的认证与 `{{var}}` 模板解析单测。
 *
 * 覆盖：Bearer / Basic / API Key（header 与 query 两种位置）各自派生的线上形态，
 * 以及 URL / header / body 里的 `{{var}}` 替换，含 join 与替换的先后（`//` 那条
 * 回归）。查询串、body 包含、cURL 一致性在同目录的其它分片里。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildFetchInit, prepareRequest } from '../http-build';
import { kv, input, prep, ENV } from './http-build-fixtures';
import type { AuthConfig, Environment, RequestBody } from '../../types/domain';
// ============================================================
// C.1 认证 —— Bearer
// ============================================================

describe('http-build: auth (Bearer)', () => {
  it('adds Authorization: Bearer <value>', () => {
    const { init } = buildFetchInit(
      input({ auth: { kind: 'bearer', bearer: 'eyJ.tok' } as AuthConfig }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer eyJ.tok');
  });

  it('resolves {{token}} in bearer against env', () => {
    const { init } = buildFetchInit(
      input({
        auth: { kind: 'bearer', bearer: '{{token}}' } as AuthConfig,
        env: ENV,
      }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer eyJ.tok');
  });

  it('leaves missing var literal when env is absent', () => {
    const { init } = buildFetchInit(
      input({ auth: { kind: 'bearer', bearer: '{{missing}}' } as AuthConfig }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer {{missing}}');
  });

  it('user Authorization header wins; auth is not appended twice', () => {
    const { init } = buildFetchInit(
      input({
        headers: [kv('Authorization', 'Bearer user-override')],
        auth: { kind: 'bearer', bearer: 'eyJ.tok' } as AuthConfig,
      }),
    );
    const h = init.headers as Record<string, string>;
    assert.equal(h['Authorization'], 'Bearer user-override');
    // 任何大小写下都不该出现重复的 "authorization" 键。
    assert.equal(h['authorization'], undefined);
  });
});

// ============================================================
// C.2 认证 —— Basic
// ============================================================

describe('http-build: auth (Basic)', () => {
  it('base64-encodes user:pass in Authorization header', () => {
    const { init } = buildFetchInit(
      input({
        auth: {
          kind: 'basic',
          basic: { username: 'alice', password: 'secret' },
        } as AuthConfig,
      }),
    );
    // btoa('alice:secret') = 'YWxpY2U6c2VjcmV0'
    assert.equal(
      (init.headers as Record<string, string>)['Authorization'],
      'Basic YWxpY2U6c2VjcmV0',
    );
  });

  it('UTF-8 encodes non-ASCII credentials before base64', () => {
    const { init } = buildFetchInit(
      input({
        auth: {
          kind: 'basic',
          basic: { username: '用户', password: '密码' },
        } as AuthConfig,
      }),
    );
    // 这里要验的是不抛异常，且解 btoa(...) 得到原始 UTF-8 字节——所以验往返，不断言某个
    // 具体的 base64 字符串（它取决于编码器细节）。
    const hdr = (init.headers as Record<string, string>)['Authorization'];
    assert.ok(hdr.startsWith('Basic '));
    const decoded = Buffer.from(hdr.slice('Basic '.length), 'base64').toString('utf-8');
    assert.equal(decoded, '用户:密码');
  });

  it('resolves {{var}} in username/password', () => {
    const { init } = buildFetchInit(
      input({
        auth: {
          kind: 'basic',
          basic: { username: '{{userId}}', password: '{{token}}' },
        } as AuthConfig,
        env: ENV,
      }),
    );
    const hdr = (init.headers as Record<string, string>)['Authorization'];
    assert.ok(hdr.startsWith('Basic '));
    const decoded = Buffer.from(hdr.slice('Basic '.length), 'base64').toString('utf-8');
    assert.equal(decoded, 'u_demo:eyJ.tok');
  });

  it('user Authorization header wins', () => {
    const { init } = buildFetchInit(
      input({
        headers: [kv('Authorization', 'Bearer override')],
        auth: {
          kind: 'basic',
          basic: { username: 'a', password: 'b' },
        } as AuthConfig,
      }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer override');
  });
});

// ============================================================
// C.3 认证 —— API Key（header / query）
// ============================================================

describe('http-build: auth (API Key)', () => {
  it('in header mode: emits <key>: <value>', () => {
    const { init } = buildFetchInit(
      input({
        auth: {
          kind: 'apikey',
          apikey: { key: 'X-API-Key', value: 'k_123', inLocation: 'header' },
        } as AuthConfig,
      }),
    );
    assert.equal((init.headers as Record<string, string>)['X-API-Key'], 'k_123');
  });

  it('in header mode: resolves {{apiKey}} from env', () => {
    const { init } = buildFetchInit(
      input({
        auth: {
          kind: 'apikey',
          apikey: { key: 'X-API-Key', value: '{{apiKey}}', inLocation: 'header' },
        } as AuthConfig,
        env: ENV,
      }),
    );
    assert.equal((init.headers as Record<string, string>)['X-API-Key'], 'k_123');
  });

  it('user has same header key → user wins, no duplicate', () => {
    const { init } = buildFetchInit(
      input({
        headers: [kv('X-API-Key', 'override')],
        auth: {
          kind: 'apikey',
          apikey: { key: 'X-API-Key', value: 'k_123', inLocation: 'header' },
        } as AuthConfig,
      }),
    );
    const h = init.headers as Record<string, string>;
    assert.equal(h['X-API-Key'], 'override');
    assert.equal(h['x-api-key'], undefined);
  });

  it('in query mode: appended to URL', () => {
    const { url, init } = buildFetchInit(
      input({
        auth: {
          kind: 'apikey',
          apikey: { key: 'apiKey', value: 'k_123', inLocation: 'query' },
        } as AuthConfig,
      }),
    );
    assert.equal(url, 'http://api.example.com/x?apiKey=k_123');
    assert.equal((init.headers as Record<string, string>)['apiKey'], undefined);
    assert.equal((init.headers as Record<string, string>)['X-API-Key'], undefined);
  });

  it('in query mode: appended with & when URL already has ?', () => {
    const { url } = buildFetchInit(
      input({
        url: 'http://api.example.com/x?foo=bar',
        auth: {
          kind: 'apikey',
          apikey: { key: 'apiKey', value: 'k_123', inLocation: 'query' },
        } as AuthConfig,
      }),
    );
    assert.equal(url, 'http://api.example.com/x?foo=bar&apiKey=k_123');
  });

  it('in query mode: URL-encodes key and value', () => {
    const { url } = buildFetchInit(
      input({
        auth: {
          kind: 'apikey',
          apikey: { key: 'a key', value: 'x&y', inLocation: 'query' },
        } as AuthConfig,
      }),
    );
    assert.equal(url, 'http://api.example.com/x?a%20key=x%26y');
  });
});

// ============================================================
// D. 模板解析（URL / header / body）
// ============================================================

describe('http-build: {{var}} template resolution', () => {
  it('resolves {{var}} in URL', () => {
    const { url } = buildFetchInit(
      input({ url: 'http://api.example.com/{{site}}/users', env: ENV }),
    );
    assert.equal(url, 'http://api.example.com/cn/users');
  });

  it('resolves {{var}} in header value', () => {
    const { init } = buildFetchInit(input({ headers: [kv('X-Region', '{{site}}')], env: ENV }));
    assert.equal((init.headers as Record<string, string>)['X-Region'], 'cn');
  });

  it('leaves missing var literal (no env at all)', () => {
    const { url } = buildFetchInit(input({ url: 'http://api.example.com/{{missing}}/x' }));
    assert.equal(url, 'http://api.example.com/{{missing}}/x');
  });

  it('resolves {{var}} in body content, matching Rust resolve_endpoint', () => {
    // body 与其他字段一样被模板化，因为 `resolve_endpoint` 一直如此——应用与它自己的
    // 后端不该不一致。这条断言替换本身；下一条断言无可替换时载荷逐字节不变。
    const body: RequestBody = {
      mode: 'json',
      content: '{"site":"{{site}}","u":"{{userId}}"}',
    };
    const { init } = buildFetchInit(input({ body, env: ENV }));
    assert.equal(init.body, '{"site":"cn","u":"u_demo"}');
    // 仍是合法 JSON——这次替换恰好是安全的。
    assert.deepEqual(JSON.parse(init.body as string), { site: 'cn', u: 'u_demo' });
  });

  it('leaves body content byte-identical when it carries no marker', () => {
    const body: RequestBody = { mode: 'json', content: '{"a": 1, "b": [2, 3]}' };
    const { init } = buildFetchInit(input({ body, env: ENV }));
    assert.equal(init.body, '{"a": 1, "b": [2, 3]}');
  });
});

// ============================================================
// D2. join / 替换的先后 —— `//` 那条回归
// ============================================================

/**
 * `prepareRequest` 刻意在替换 `{{var}}` **之前**先 join base URL：环境自己的
 * base URL 也可能是模板，替换必须能看见它。代价是 `joinBaseUrl` 在还不知道
 * `{{var}}` 会变成什么时就插入了分隔符，于是值以 `/` 开头的变量会得到
 * `http://host:9200//api`——对多数服务端那是另一条路由。
 *
 * 这些用例走真正的 `prepareRequest` 而非只用 `joinBaseUrl`，因为这个顺序只存在于
 * 组装后的流水线里。Rust 侧对应的用例在
 * `crates/simple-post-mcp/src/variable_fill_test.rs`；两者必须一致，否则预览描述的
 * URL 没人会发。
 */
describe('http-build: baseUrl join + {{var}} expansion never yields //', () => {
  const envWith = (vars: Record<string, string>): Environment => ({
    ...ENV,
    variables: { ...ENV.variables, ...vars },
  });

  it('repairs the double slash when the path is nothing but the marker', () => {
    const { url } = prepareRequest(
      prep({ baseUrl: 'http://host:9200', urlPath: '{{var}}', env: envWith({ var: '/api' }) }),
    );
    assert.equal(url, 'http://host:9200/api');
  });

  it('is unchanged when the baseUrl itself ends in a slash', () => {
    const { url } = prepareRequest(
      prep({ baseUrl: 'http://host:9200/', urlPath: '{{var}}', env: envWith({ var: '/api' }) }),
    );
    assert.equal(url, 'http://host:9200/api');
  });

  it('repairs the boundary when the path continues past the marker', () => {
    const { url } = prepareRequest(
      prep({
        baseUrl: 'http://host:9200',
        urlPath: '{{var}}/users',
        env: envWith({ var: '/api' }),
      }),
    );
    assert.equal(url, 'http://host:9200/api/users');
  });

  it('does not eat the separator when the value has no leading slash', () => {
    const { url } = prepareRequest(
      prep({ baseUrl: 'http://host:9200', urlPath: '{{var}}', env: envWith({ var: 'api' }) }),
    );
    assert.equal(url, 'http://host:9200/api');
  });

  it('keeps the https:// scheme intact', () => {
    const { url } = prepareRequest(
      prep({ baseUrl: 'https://host:9200', urlPath: '{{var}}', env: envWith({ var: '/api' }) }),
    );
    assert.equal(url, 'https://host:9200/api');
  });

  it('still lets a schemed urlPath win outright over the baseUrl', () => {
    const { url } = prepareRequest(
      prep({
        baseUrl: 'https://custom.example.com',
        urlPath: 'https://other.example.com/users/1',
      }),
    );
    assert.equal(url, 'https://other.example.com/users/1');
  });

  it('keeps an unresolved marker and reports it', () => {
    const r = prepareRequest(prep({ baseUrl: 'http://host:9200', urlPath: '{{gone}}', env: ENV }));
    assert.equal(r.url, 'http://host:9200/{{gone}}');
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('still appends params with ? and & after collapsing', () => {
    // 折叠必须跑在追加查询串之前，且不能吃掉 URL 里已有的查询串——`?`/`&` 的选择要
    // 读 URL，所以 path 被弄坏不会是唯一的症状
    const fresh = prepareRequest(
      prep({
        baseUrl: 'http://host:9200',
        urlPath: '{{var}}',
        env: envWith({ var: '/api' }),
        params: [kv('page', '1')],
      }),
    );
    assert.equal(fresh.url, 'http://host:9200/api?page=1');

    const existing = prepareRequest(
      prep({
        baseUrl: 'http://host:9200',
        urlPath: '{{var}}?foo=bar',
        env: envWith({ var: '/api' }),
        params: [kv('page', '1')],
      }),
    );
    assert.equal(existing.url, 'http://host:9200/api?foo=bar&page=1');
  });

  it('leaves // inside query data alone', () => {
    const { url } = prepareRequest(
      prep({
        baseUrl: 'http://host:9200',
        urlPath: '{{var}}?redirect=https://a//b',
        env: envWith({ var: '/api' }),
      }),
    );
    assert.equal(url, 'http://host:9200/api?redirect=https://a//b');
  });
});
