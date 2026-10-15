/**
 * `prepareRequest`（唯一的构造核）的单测，以及两个旧适配器与它的逐字节一致性。
 *
 * 覆盖：base URL 的 join、查询串组装、有序 header 元组、body 与 `missingVars`，
 * 外加 `buildFetchInit` / `buildCurlCommand` 必须精确复现 `prepareRequest` 的产物
 * ——包括「预览与发送不可能漂移」那条不变式。适配器自身的用例在同目录的其它分片里。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildFetchInit, buildCurlCommand, prepareRequest, preparedToCurl } from '../http-build';
import { kv, input, prep, headerMap, ENV } from './http-build-fixtures';
import type { AuthConfig, Environment } from '../../types/domain';
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
