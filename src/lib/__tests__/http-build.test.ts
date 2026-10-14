/**
 * `http-build.ts`（共享的线上形态构造器）的纯单测。通过 `npm test` 运行
 * （它调用 `tsx --test`）。
 *
 * 覆盖矩阵：
 *   A. 查询串拼装
 *   B. header 顺序 / 自动 Content-Type / 自动 Accept
 *   C. 认证：Bearer / Basic / API Key（header 与 query）
 *   D. URL 与 header 的 `{{var}}` 模板解析
 *   E. body 是否包含（none / json / form / raw，空内容保护）
 *   F. HTTP 方法保真（全部 7 个动词）
 *   G. cURL 一致性 —— `buildCurlCommand` 与 `buildFetchInit` 同源
 *   H. 边界：特殊字符、UTF-8 凭据、与用户 header 的去重
 *
 * 这些测试**不**触及 `fetch` 本身；真实网络层的集成覆盖在 `http.test.ts`。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  buildFetchInit,
  buildCurlCommand,
  prepareRequest,
  preparedToCurl,
  type BuildInput,
  type PrepareRequestInput,
} from '../http-build';
import type {
  AuthConfig,
  Environment,
  HttpMethod,
  KeyValue,
  RequestBody,
} from '../../types/domain';

// ---------- 共用夹具 ----------

const ENV: Environment = {
  id: 'env_test',
  projectId: 'p_test',
  name: 'test',
  baseUrls: {},
  // (模块 × 服务) 的 URL 层——`prepareRequest` 并不认这个概念（它收到的是已解析好的
  // `baseUrl`），但该字段没有默认值，所以夹具必须点名它
  moduleBaseUrls: {},
  variables: {
    token: 'eyJ.tok',
    userId: 'u_demo',
    apiKey: 'k_123',
    site: 'cn',
  },
  isActive: true,
};

function kv(key: string, value: string, enabled = true): KeyValue {
  return { id: `k_${key}`, key, value, enabled };
}

function input(overrides: Partial<BuildInput> = {}): BuildInput {
  return {
    method: 'GET',
    url: 'http://api.example.com/x',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    ...overrides,
  };
}

/** 同样的夹具形状，但拆成 `prepareRequest`（与 base URL 选择器）真正吃的那对
 *  path / base URL。 */
function prep(overrides: Partial<PrepareRequestInput> = {}): PrepareRequestInput {
  return {
    method: 'GET',
    urlPath: '/x',
    baseUrl: '',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    ...overrides,
  };
}

/** 元组列表 → 查找表，供「某个 header 存不存在」这类断言用。 */
function headerMap(tuples: ReadonlyArray<readonly [string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of tuples) out[k] = v;
  return out;
}

// ============================================================
// A. 查询串拼装
// ============================================================

describe('http-build: query string', () => {
  it('leaves URL unchanged when params is empty', () => {
    const { url } = buildFetchInit(input({ params: [] }));
    assert.equal(url, 'http://api.example.com/x');
  });

  it('appends a single param with ?', () => {
    const { url } = buildFetchInit(input({ params: [kv('page', '1')] }));
    assert.equal(url, 'http://api.example.com/x?page=1');
  });

  it('joins multiple params with &', () => {
    const { url } = buildFetchInit(
      input({ params: [kv('page', '1'), kv('size', '20'), kv('sort', 'desc')] }),
    );
    assert.equal(url, 'http://api.example.com/x?page=1&size=20&sort=desc');
  });

  it('uses & when URL already has ?', () => {
    const { url } = buildFetchInit(
      input({ url: 'http://api.example.com/x?foo=bar', params: [kv('page', '1')] }),
    );
    assert.equal(url, 'http://api.example.com/x?foo=bar&page=1');
  });

  it('uses & when URL already has multiple query params', () => {
    const { url } = buildFetchInit(
      input({ url: 'http://api.example.com/x?foo=bar&baz=qux', params: [kv('page', '1')] }),
    );
    assert.equal(url, 'http://api.example.com/x?foo=bar&baz=qux&page=1');
  });

  it('skips disabled params', () => {
    const { url } = buildFetchInit(input({ params: [kv('a', '1'), kv('b', '2', false)] }));
    assert.equal(url, 'http://api.example.com/x?a=1');
  });

  it('skips params with empty key', () => {
    const { url } = buildFetchInit(input({ params: [kv('', 'orphan'), kv('kept', 'yes')] }));
    assert.equal(url, 'http://api.example.com/x?kept=yes');
  });

  it('skips params with empty value', () => {
    const { url } = buildFetchInit(input({ params: [kv('a', ''), kv('b', '2')] }));
    assert.equal(url, 'http://api.example.com/x?b=2');
  });

  it('URL-encodes special characters in key and value', () => {
    const { url } = buildFetchInit(input({ params: [kv('a b', 'x&y=z+1')] }));
    // encodeURIComponent: ' ' -> '%20', '&' -> '%26', '=' -> '%3D', '+' -> '%2B'
    assert.equal(url, 'http://api.example.com/x?a%20b=x%26y%3Dz%2B1');
  });

  it('URL-encodes unicode in value', () => {
    const { url } = buildFetchInit(input({ params: [kv('q', '中文')] }));
    // '中' -> %E4%B8%AD, '文' -> %E6%96%87
    assert.equal(url, 'http://api.example.com/x?q=%E4%B8%AD%E6%96%87');
  });
});

// ============================================================
// B. header 顺序 / 自动 Content-Type / 自动 Accept
// ============================================================

describe('http-build: headers', () => {
  it('emits a user header alongside the auto-injected Accept', () => {
    const { init } = buildFetchInit(input({ headers: [kv('X-Trace', 'abc')] }));
    assert.deepEqual(init.headers, {
      'X-Trace': 'abc',
      Accept: 'application/json',
    });
  });

  it('skips disabled headers', () => {
    const { init } = buildFetchInit(input({ headers: [kv('A', '1', false), kv('B', '2')] }));
    assert.deepEqual(init.headers, { B: '2', Accept: 'application/json' });
  });

  it('skips headers with empty key', () => {
    const { init } = buildFetchInit(input({ headers: [kv('', 'orphan'), kv('B', '2')] }));
    assert.deepEqual(init.headers, { B: '2', Accept: 'application/json' });
  });

  it('preserves the user-set Content-Type (case-insensitive)', () => {
    const { init } = buildFetchInit(
      input({
        body: { mode: 'json', content: '{"a":1}' },
        headers: [kv('content-type', 'application/vnd.custom+json')],
      }),
    );
    // 用户值优先；自动补的 application/json 不得覆盖它。
    assert.equal(
      (init.headers as Record<string, string>)['content-type'],
      'application/vnd.custom+json',
    );
    assert.equal((init.headers as Record<string, string>)['Content-Type'], undefined);
  });

  it('auto-adds Content-Type for json mode when user has none', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'json', content: '{}' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/json');
  });

  it('auto-adds Content-Type for form mode', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'form', content: 'a=1' } }));
    assert.equal(
      (init.headers as Record<string, string>)['Content-Type'],
      'application/x-www-form-urlencoded',
    );
  });

  it('auto-adds Content-Type for raw mode', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'raw', content: 'hello' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'text/plain');
  });

  it('does NOT auto-add Content-Type when body.mode is none', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'none', content: '' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], undefined);
  });

  it('auto-adds Content-Type even when json content is empty (mode implies a body shape)', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'json', content: '' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/json');
  });

  it('auto-adds Accept: application/json when user has none', () => {
    const { init } = buildFetchInit(input());
    assert.equal((init.headers as Record<string, string>)['Accept'], 'application/json');
  });

  it('respects user-set Accept (case-insensitive)', () => {
    const { init } = buildFetchInit(input({ headers: [kv('accept', 'text/html')] }));
    assert.equal((init.headers as Record<string, string>)['accept'], 'text/html');
    assert.equal((init.headers as Record<string, string>)['Accept'], undefined);
  });

  it('keeps multiple user headers in declared order', () => {
    const { init } = buildFetchInit(
      input({
        body: { mode: 'json', content: '{}' },
        headers: [kv('X-Trace', '1'), kv('X-Other', '2')],
      }),
    );
    const keys = Object.keys(init.headers as Record<string, string>);
    // 确切插入顺序取决于实现里 header 数组收成 record 的方式，但这两个用户 header
    // 必须排在任何自动补的（Content-Type、Accept）之前
    assert.ok(keys.indexOf('X-Trace') < keys.indexOf('Content-Type'));
    assert.ok(keys.indexOf('X-Other') < keys.indexOf('Accept'));
  });
});

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
    // 这里要验的是不抛异常，且解 btoa(...) 得到原始 UTF-8 字节——所以验往返而不钉死
    // 某个 base64 字符串（它取决于编码器细节）。
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

// ============================================================
// I. prepareRequest —— 唯一的构造核
//
// 发送端与 cURL 预览现在共用 `prepareRequest`。下面这些用例覆盖这个安排里最容易
// 静默出错的几处：base URL 没 join、header 被 `Record` 压扁，以及——它存在的
// 理由——发送前无从追问「还有哪些没解析出来」。
// ============================================================

describe('prepareRequest: base URL join', () => {
  it('joins the path onto the base URL', () => {
    const r = prepareRequest(prep({ baseUrl: 'https://api.example.com', urlPath: '/users/1' }));
    assert.equal(r.url, 'https://api.example.com/users/1');
  });

  it('inserts the slash when the path has none', () => {
    const r = prepareRequest(prep({ baseUrl: 'https://api.example.com', urlPath: 'users/1' }));
    assert.equal(r.url, 'https://api.example.com/users/1');
  });

  it('collapses a trailing slash on the base URL', () => {
    const r = prepareRequest(prep({ baseUrl: 'https://api.example.com/', urlPath: 'users/1' }));
    assert.equal(r.url, 'https://api.example.com/users/1');
  });

  it('lets a path carrying its own scheme win (自定义 mode)', () => {
    // 选择器里的「自定义」行就是这个：URL 字段装的是完整地址，再前缀环境 base 会
    // 把它弄坏
    const r = prepareRequest(
      prep({ baseUrl: 'https://custom.example.com', urlPath: 'https://other/x' }),
    );
    assert.equal(r.url, 'https://other/x');
  });

  it('leaves the path alone when there is no base URL', () => {
    const r = prepareRequest(prep({ baseUrl: '', urlPath: '/users/1' }));
    assert.equal(r.url, '/users/1');
  });

  it('resolves {{var}} after joining, including inside the base URL', () => {
    // 先 join 再解析，而不是先解析再 join：环境自己的 base URL 也是模板，只解析 path
    // 会让 `https://{{host}}/v1` 留在半替换状态
    const env = {
      variables: { host: 'api.example.com', site: 'cn' },
      baseUrls: {},
    } as unknown as Environment;
    const r = prepareRequest(
      prep({ baseUrl: 'https://{{host}}', urlPath: '/{{site}}/users', env }),
    );
    assert.equal(r.url, 'https://api.example.com/cn/users');
  });
});

describe('prepareRequest: query assembly', () => {
  it('appends params to a URL that already has a query', () => {
    const r = prepareRequest(prep({ urlPath: '/x?foo=bar', params: [kv('page', '1')] }));
    assert.equal(r.url, '/x?foo=bar&page=1');
  });

  it('appends an apikey-in-query after the params', () => {
    const r = prepareRequest(
      prep({
        params: [kv('page', '1')],
        auth: {
          kind: 'apikey',
          apikey: { key: 'k', value: 'v', inLocation: 'query' },
        } as AuthConfig,
      }),
    );
    assert.equal(r.url, '/x?page=1&k=v');
  });

  it('resolves {{var}} in a param value before encoding it', () => {
    const r = prepareRequest(
      prep({ urlPath: '/x', params: [kv('q', '{{site}}'), kv('id', '{{userId}}')], env: ENV }),
    );
    assert.equal(r.url, '/x?q=cn&id=u_demo');
    assert.deepEqual(r.missingVars, []);
  });

  it('leaves a param key verbatim, mirroring Rust resolve_endpoint', () => {
    // Rust 只替换值（`p.key.clone()`），这里若解析 key，预览与真实发送就会不一致
    const r = prepareRequest(prep({ urlPath: '/x', params: [kv('{{site}}', 'v')], env: ENV }));
    assert.equal(r.url, '/x?%7B%7Bsite%7D%7D=v');
  });

  it('reports a param marker it could not resolve', () => {
    const r = prepareRequest(prep({ urlPath: '/x', params: [kv('q', '{{gone}}')], env: ENV }));
    assert.deepEqual(r.missingVars, ['gone']);
    assert.equal(r.url, '/x?q=%7B%7Bgone%7D%7D');
  });

  it('ignores markers on a disabled param', () => {
    const r = prepareRequest(
      prep({ urlPath: '/x', params: [kv('q', '{{gone}}', false)], env: ENV }),
    );
    assert.deepEqual(r.missingVars, []);
    assert.equal(r.url, '/x');
  });

  it('drops a param whose variable resolves to empty', () => {
    const r = prepareRequest(
      prep({
        urlPath: '/x',
        params: [kv('q', '{{blank}}'), kv('keep', '1')],
        env: { ...ENV, variables: { ...ENV.variables, blank: '' } },
      }),
    );
    assert.equal(r.url, '/x?keep=1');
  });
});

describe('prepareRequest: headers are ordered tuples', () => {
  it('emits user headers first, then Content-Type / Accept / auth', () => {
    const r = prepareRequest(
      prep({
        body: { mode: 'json', content: '{"a":1}' },
        headers: [kv('X-Trace', 'abc')],
        auth: { kind: 'bearer', bearer: 'tok' } as AuthConfig,
      }),
    );
    assert.deepEqual(r.headers, [
      ['X-Trace', 'abc'],
      ['Content-Type', 'application/json'],
      ['Accept', 'application/json'],
      ['Authorization', 'Bearer tok'],
    ]);
  });

  it('keeps a repeated header name as two tuples', () => {
    // 线上形态之所以是列表而不是 Record：HTTP 允许 header 重复，收成 `Record`
    // 会丢掉一个
    const r = prepareRequest(prep({ headers: [kv('Set-Cookie', 'a=1'), kv('Set-Cookie', 'b=2')] }));
    assert.deepEqual(r.headers, [
      ['Set-Cookie', 'a=1'],
      ['Set-Cookie', 'b=2'],
      ['Accept', 'application/json'],
    ]);
  });

  it('derives a Basic Authorization header', () => {
    const r = prepareRequest(
      prep({
        auth: { kind: 'basic', basic: { username: 'alice', password: 'secret' } } as AuthConfig,
      }),
    );
    assert.equal(headerMap(r.headers)['Authorization'], 'Basic YWxpY2U6c2VjcmV0');
  });

  it('derives an apikey-as-header', () => {
    const r = prepareRequest(
      prep({
        auth: {
          kind: 'apikey',
          apikey: { key: 'X-API-Key', value: 'k_123', inLocation: 'header' },
        } as AuthConfig,
      }),
    );
    assert.equal(headerMap(r.headers)['X-API-Key'], 'k_123');
  });

  it('does not override a user header of the same name (case-insensitive)', () => {
    const r = prepareRequest(
      prep({
        headers: [kv('authorization', 'Bearer user')],
        auth: { kind: 'bearer', bearer: 'auth-v1' } as AuthConfig,
      }),
    );
    // 恰好一条 authorization 条目，且是用户那条
    const auths = r.headers.filter(([k]) => k.toLowerCase() === 'authorization');
    assert.deepEqual(auths, [['authorization', 'Bearer user']]);
  });

  it('skips disabled headers and headers with an empty key', () => {
    const r = prepareRequest(
      prep({ headers: [kv('A', '1', false), kv('', 'orphan'), kv('B', '2')] }),
    );
    assert.deepEqual(r.headers, [
      ['B', '2'],
      ['Accept', 'application/json'],
    ]);
  });
});

describe('prepareRequest: body', () => {
  it('is null when mode is none', () => {
    assert.equal(prepareRequest(prep()).body, null);
    assert.equal(prepareRequest(prep({ body: { mode: 'none', content: 'ignored' } })).body, null);
  });

  it('is null when the content is empty, even if the mode implies one', () => {
    assert.equal(prepareRequest(prep({ body: { mode: 'json', content: '' } })).body, null);
  });

  it('carries the content verbatim, without {{var}} expansion', () => {
    // 这里没传 env，所以标记无处可解析、原样留在载荷里并被 `missingVars` 报告。
    // 有 env 时 body 与其他字段一样被模板化（见上面那条用例）。
    const r = prepareRequest(prep({ body: { mode: 'json', content: '{"u":"{{userId}}"}' } }));
    assert.equal(r.body, '{"u":"{{userId}}"}');
  });
});

describe('prepareRequest: missingVars', () => {
  it('is empty for a fully resolved request', () => {
    const r = prepareRequest(
      prep({
        urlPath: '/{{site}}/users',
        headers: [kv('X-Trace', '{{site}}')],
        auth: { kind: 'bearer', bearer: '{{token}}' } as AuthConfig,
        body: { mode: 'json', content: '{"u":"u_demo"}' },
        env: ENV,
      }),
    );
    assert.deepEqual(r.missingVars, []);
  });

  it('reports a marker from the URL', () => {
    const r = prepareRequest(prep({ urlPath: '/u/{{gone}}', env: ENV }));
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('reports a marker from the base URL', () => {
    const r = prepareRequest(prep({ baseUrl: 'https://{{gone}}.com', urlPath: '/x', env: ENV }));
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('reports a marker from an enabled header value', () => {
    const r = prepareRequest(prep({ headers: [kv('X-Region', '{{gone}}')], env: ENV }));
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('ignores markers on headers that are not on the wire', () => {
    // 未启用的 header 不会发出，所以它里面的变量不算这次请求的问题——报出来是误报
    const r = prepareRequest(
      prep({ headers: [kv('X-Region', '{{gone}}', false), kv('', '{{alsoGone}}')], env: ENV }),
    );
    assert.deepEqual(r.missingVars, []);
  });

  it('reports a marker from the body only when it did not resolve', () => {
    // body 会被模板化，所以环境已定义的变量不构成警告——它已经在载荷里了；未定义
    // 的才是，也正是 UI 需要在发送前提示出来的那种
    const resolved = prepareRequest(
      prep({ body: { mode: 'json', content: '{"u":"{{userId}}"}' }, env: ENV }),
    );
    assert.equal(resolved.body, '{"u":"u_demo"}');
    assert.deepEqual(resolved.missingVars, []);

    const unresolved = prepareRequest(
      prep({ body: { mode: 'json', content: '{"u":"{{gone}}"}' }, env: ENV }),
    );
    assert.equal(unresolved.body, '{"u":"{{gone}}"}');
    assert.deepEqual(unresolved.missingVars, ['gone']);
  });

  it('decides body inclusion on the authored content, not the resolved one', () => {
    // body 只是一个解析为空串的变量时，仍会发一个（空）body。模板化之后再判断会把
    // 这个请求的 body 悄悄丢掉从而改变请求。与 `resolve_endpoint` 同序
    const r = prepareRequest(
      prep({
        body: { mode: 'json', content: '{{blank}}' },
        env: { ...ENV, variables: { ...ENV.variables, blank: '' } },
      }),
    );
    assert.equal(r.body, '');
    assert.deepEqual(r.missingVars, []);
  });

  it('does not scan the body of a request that sends none', () => {
    const r = prepareRequest(prep({ body: { mode: 'none', content: '{{gone}}' } }));
    assert.equal(r.body, null);
    assert.deepEqual(r.missingVars, []);
  });

  it('reports a marker from a bearer token', () => {
    const r = prepareRequest(prep({ auth: { kind: 'bearer', bearer: '{{gone}}' } as AuthConfig }));
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('reports markers from basic username and password', () => {
    const r = prepareRequest(
      prep({
        auth: {
          kind: 'basic',
          basic: { username: '{{userId}}', password: '{{gone}}' },
        } as AuthConfig,
        env: ENV,
      }),
    );
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('reports markers from apikey key and value, in header location', () => {
    const r = prepareRequest(
      prep({
        auth: {
          kind: 'apikey',
          apikey: { key: '{{apiKey}}', value: '{{gone}}', inLocation: 'header' },
        } as AuthConfig,
        env: ENV,
      }),
    );
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('reports markers from an apikey in query location too', () => {
    const r = prepareRequest(
      prep({
        auth: {
          kind: 'apikey',
          apikey: { key: '{{apiKey}}', value: '{{gone}}', inLocation: 'query' },
        } as AuthConfig,
        env: ENV,
      }),
    );
    assert.deepEqual(r.missingVars, ['gone']);
  });

  it('dedupes across sources and keeps a stable order', () => {
    const r = prepareRequest(
      prep({
        urlPath: '/u/{{a}}',
        headers: [kv('X-Region', '{{b}}')],
        body: { mode: 'json', content: '{"c":"{{c}}"}' },
        auth: { kind: 'bearer', bearer: '{{a}}' } as AuthConfig,
      }),
    );
    // 先 URL，再 header，再 body，再 auth——而两处出现的 `{{a}}` 只列一次
    assert.deepEqual(r.missingVars, ['a', 'b', 'c']);
  });

  it('does not report a marker that a value merely contains', () => {
    // 一个已解析变量的*值*里含 `{{notavar}}` 并不使它本身成为缺失变量——替换是非递归
    // 的单趟，注入进来的文本是数据，不是第二轮替换
    const r = prepareRequest(
      prep({
        urlPath: '/{{a}}',
        env: { variables: { a: '{{b}}' }, baseUrls: {} } as unknown as Environment,
      }),
    );
    assert.equal(r.url, '/{{b}}');
    assert.deepEqual(r.missingVars, []);
  });
});

// ============================================================
// J. 两个适配器保持逐字节一致
//
// fetch / cURL 辅助函数是薄包装。这里钉住它们精确复现既有产物——空 base URL 是
// 恒等，所以一个本来就是完整 URL 的 `BuildInput.url` 必须原样出来。
// ============================================================

describe('prepareRequest is what the legacy adapters build', () => {
  it('buildFetchInit matches prepareRequest for a full URL', () => {
    const i = input({
      url: 'http://api.example.com/x?foo=bar',
      params: [kv('page', '1')],
      headers: [kv('X-Trace', 'abc')],
      body: { mode: 'json', content: '{"a":1}' },
      auth: { kind: 'bearer', bearer: 'tok' } as AuthConfig,
    });
    const p = prepareRequest(prep({ ...i, urlPath: i.url }));
    const f = buildFetchInit(i);
    assert.equal(f.url, p.url);
    assert.deepEqual(Object.entries(f.init.headers as Record<string, string>), p.headers);
    assert.equal(f.init.body, p.body);
  });

  it('buildCurlCommand still renders the same line', () => {
    const i = input({
      method: 'POST',
      url: 'https://api.example.com/cashier',
      headers: [kv('X-Trace', "it's")],
      body: { mode: 'json', content: '{"a":1}' },
    });
    assert.equal(
      buildCurlCommand(i),
      [
        `curl -X POST 'https://api.example.com/cashier'`,
        `  -H 'X-Trace: it'\\''s'`,
        `  -H 'Content-Type: application/json'`,
        `  -H 'Accept: application/json'`,
        `  --data-raw '{"a":1}'`,
      ].join(' \\\n'),
    );
  });

  it('an empty base URL leaves a complete URL alone', () => {
    // `buildFetchInit` 的契约：`BuildInput.url` 就是完整 URL
    assert.equal(buildFetchInit(input()).url, 'http://api.example.com/x');
    assert.equal(
      prepareRequest(prep({ urlPath: 'http://api.example.com/x' })).url,
      'http://api.example.com/x',
    );
  });
});

/**
 * 整个重构要立的那条不变式：用户复制到的，就是服务端收到的。
 *
 * `sendPreparedRequest` 与 `preparedToCurl` 拿到的是**同一个** `PreparedRequest`
 * 对象，于是不存在第二份线上形态组装还能跟它不一致。这些用例把同一个请求同时过
 * 两个公开入口并要求逐字节相等——日后若有人给 `buildCurlCommand` 加了
 * `prepareRequest` 没有的 header 规则，或预览又开始自己 join base URL，拦下它的
 * 就是这里。
 */
describe('prepareRequest → preparedToCurl: preview and send cannot drift', () => {
  const full = prep({
    method: 'POST',
    urlPath: '/cashier/settle',
    baseUrl: 'https://api.example.com',
    params: [
      { id: 'p1', key: 'shop', value: 'a b', enabled: true },
      { id: 'p2', key: 'off', value: 'x', enabled: false },
    ],
    headers: [{ id: 'h1', key: 'X-Trace', value: "it's", enabled: true }],
    body: { mode: 'json', content: '{"amount":1}' },
    auth: { kind: 'bearer', bearer: 'tok' },
  });

  it('matches buildCurlCommand for the same request', () => {
    assert.equal(
      preparedToCurl(prepareRequest(full)),
      buildCurlCommand(input({ ...full, url: 'https://api.example.com/cashier/settle' })),
    );
  });

  it('emits the joined URL, never the bare path', () => {
    // 预览必须发出 join 后的 URL，绝不是裸 path：预览若只拿 `EndpointDetail.url`
    // （它只存 path），就会为一个实际去了 `https://api.example.com/cashier` 的请求
    // 打印出 `curl -X POST '/cashier'`
    const curl = preparedToCurl(prepareRequest(full));
    assert.match(curl, /^curl -X POST 'https:\/\/api\.example\.com\/cashier\/settle\?shop=a%20b'/);
  });

  it('carries the auth-derived header a real send would send', () => {
    const headers = prepareRequest(full).headers;
    assert.ok(headers.some(([k, v]) => k === 'Authorization' && v === 'Bearer tok'));
  });

  it('omits --data-raw when there is no body', () => {
    assert.doesNotMatch(preparedToCurl(prepareRequest(prep())), /--data-raw/);
  });
});
