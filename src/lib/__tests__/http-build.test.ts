/**
 * Pure unit tests for `http-build.ts` — the shared wire-shape builder.
 *
 * Run via `npm test` (which invokes `tsx --test`).
 *
 * Coverage matrix (mapped to PLAN §2 sendRequest 关键行为):
 *   A. Query string assembly
 *   B. Header ordering / auto Content-Type / auto Accept
 *   C. Auth: Bearer / Basic / API Key (header + query)
 *   D. URL & header `{{var}}` template resolution
 *   E. Body inclusion (none / json / form / raw, empty-content guard)
 *   F. HTTP method preservation (all 7 verbs)
 *   G. cURL parity — `buildCurlCommand` mirrors `buildFetchInit`
 *   H. Edge cases: special chars, UTF-8 credentials, dedupe vs user headers
 *
 * The tests do NOT exercise `fetch` itself; integration coverage of the
 * actual network layer lives in `http.test.ts`.
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

// ---------- shared fixtures ----------

const ENV: Environment = {
  id: 'env_test',
  projectId: 'p_test',
  name: 'test',
  baseUrls: {},
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

/** Same fixture shape, but split into the path/base URL pair that
 *  `prepareRequest` (and the base-URL picker) actually work with. */
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

/** Tuple list → lookup map, for "does the header exist" assertions. */
function headerMap(tuples: ReadonlyArray<readonly [string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of tuples) out[k] = v;
  return out;
}

// ============================================================
// A. Query string assembly
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
// B. Header ordering / auto Content-Type / auto Accept
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
    // User value wins; auto-application/json must NOT clobber.
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
    // The exact insertion order depends on the implementation's
    // header-array-to-record collapse, but these two user headers
    // must precede any auto-injected ones (Content-Type, Accept).
    assert.ok(keys.indexOf('X-Trace') < keys.indexOf('Content-Type'));
    assert.ok(keys.indexOf('X-Other') < keys.indexOf('Accept'));
  });
});

// ============================================================
// C.1 Auth — Bearer
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
    // No duplicate "authorization" key at any case.
    assert.equal(h['authorization'], undefined);
  });
});

// ============================================================
// C.2 Auth — Basic
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
    // The header should not throw, and decoding btoa(...) gives the
    // raw UTF-8 bytes — we verify the round-trip rather than pinning
    // a specific base64 string (which depends on encoder details).
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
// C.3 Auth — API Key (header / query)
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
// D. Template resolution (URL / header / body)
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

  it('body content is sent verbatim (no {{var}} resolution — matches curl.ts)', () => {
    // Intentional: body templating is NOT done by the builder. This
    // pins the current behavior so a future change that flips this
    // requires a conscious update to this test (and likely a follow-up
    // spec discussion with the user about whether they want body
    // templating at all).
    const body: RequestBody = {
      mode: 'json',
      content: '{"site":"{{site}}","u":"{{userId}}"}',
    };
    const { init } = buildFetchInit(input({ body, env: ENV }));
    assert.equal(init.body, '{"site":"{{site}}","u":"{{userId}}"}');
  });
});

// ============================================================
// E. Body inclusion
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
// F. HTTP method preservation (all 7 verbs)
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
// G. cURL parity
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
    // Regression net. `buildCurlCommand` only runs `resolveTemplate` over
    // the url it is given — it does NOT join a base URL. Every other test
    // in this file passes a full URL, so the path-only shape the renderer
    // actually produces (the URL input holds just the path) was never
    // covered, and "Copy as cURL" emitted `curl -X POST '/cashier'`,
    // which cannot run. HttpTester now hands the joined URL in; this test
    // pins the contract that whatever URL arrives is what gets emitted.
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
    // POSIX single-quote escape: ' → '\''
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
// H. Edge cases / dedupe across surfaces
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
// I. prepareRequest — the single construction core
//
// `prepareRequest` is what the sender and the cURL preview now share.
// The cases below are the ones that used to be silent bugs in that
// arrangement: a base URL that was never joined, a header collapsed by
// a Record, and — the reason it exists — no way to ask "what is still
// unresolved?" before sending.
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
    // The picker's 自定义 row is exactly this: the URL field holds the
    // whole address, so prefixing the environment base would corrupt it.
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
    // Join-then-resolve, not resolve-then-join: an env base URL is
    // itself templated, and resolving only the path would leave
    // `https://{{host}}/v1` half-substituted.
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
    // The reason the wire shape is a list and not a Record: HTTP allows
    // a header to repeat, and collapsing to `Record` would drop one.
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
    // Exactly one authorization entry, and it is the user's.
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
    // Matches the pre-existing builder: a JSON body is the user's text,
    // not a template. The markers are still *reported* (see below) —
    // shipped literally, but not silently.
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
    // A disabled header never ships, so its variables are not this
    // request's problem — reporting them would be a false alarm.
    const r = prepareRequest(
      prep({ headers: [kv('X-Region', '{{gone}}', false), kv('', '{{alsoGone}}')], env: ENV }),
    );
    assert.deepEqual(r.missingVars, []);
  });

  it('reports a marker from the body even when the variable exists', () => {
    // The body is not templated, so `{{userId}}` ships literally and
    // IS unresolved in the request that goes out — whether or not the
    // environment happens to define it. Reporting it here is what lets
    // the UI warn before the user hits Send.
    const r = prepareRequest(
      prep({ body: { mode: 'json', content: '{"u":"{{userId}}"}' }, env: ENV }),
    );
    assert.deepEqual(r.missingVars, ['userId']);
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
    // URL first, then headers, then body, then auth — and `{{a}}` in two
    // places is listed once.
    assert.deepEqual(r.missingVars, ['a', 'b', 'c']);
  });

  it('does not report a marker that a value merely contains', () => {
    // A resolved variable whose *value* is `{{notavar}}` is not itself
    // a missing variable — replacement is one non-recursive pass, so
    // the injected text is data, not a second round of substitution.
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
// J. The adapters stay byte-identical
//
// The fetch / cURL helpers are now thin wrappers. These pin that they
// reproduce the legacy product exactly — an empty base URL is the
// identity, so a `BuildInput.url` that is already a full URL must come
// out untouched.
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
    // `buildFetchInit`'s contract: `BuildInput.url` is the whole URL.
    assert.equal(buildFetchInit(input()).url, 'http://api.example.com/x');
    assert.equal(
      prepareRequest(prep({ urlPath: 'http://api.example.com/x' })).url,
      'http://api.example.com/x',
    );
  });
});

/**
 * The invariant the whole refactor exists to establish: what the user
 * copies is what the server receives.
 *
 * `sendPreparedRequest` and `preparedToCurl` are handed the *same*
 * `PreparedRequest` object, so there is no second assembly of the wire
 * shape left to disagree with. These tests pin that down by rendering one
 * request through both public entry points and demanding byte equality —
 * if someone later teaches `buildCurlCommand` a header rule that
 * `prepareRequest` does not have, or the preview starts re-joining the
 * base URL, this is what notices.
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
    // The exact regression this replaced: the preview used to be built
    // from `EndpointDetail.url`, which holds only the path, so it printed
    // `curl -X POST '/cashier'` for a request that had gone to
    // `https://api.example.com/cashier`.
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
