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

import { buildFetchInit, buildCurlCommand, type BuildInput } from './http-build';
import type {
  AuthConfig,
  Environment,
  HttpMethod,
  KeyValue,
  RequestBody,
} from '../types/domain';

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
    auth: { type: 'none' },
    ...overrides,
  };
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
    const { url } = buildFetchInit(
      input({ params: [kv('page', '1')] }),
    );
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
    const { url } = buildFetchInit(
      input({ params: [kv('a', '1'), kv('b', '2', false)] }),
    );
    assert.equal(url, 'http://api.example.com/x?a=1');
  });

  it('skips params with empty key', () => {
    const { url } = buildFetchInit(
      input({ params: [kv('', 'orphan'), kv('kept', 'yes')] }),
    );
    assert.equal(url, 'http://api.example.com/x?kept=yes');
  });

  it('skips params with empty value', () => {
    const { url } = buildFetchInit(
      input({ params: [kv('a', ''), kv('b', '2')] }),
    );
    assert.equal(url, 'http://api.example.com/x?b=2');
  });

  it('URL-encodes special characters in key and value', () => {
    const { url } = buildFetchInit(
      input({ params: [kv('a b', 'x&y=z+1')] }),
    );
    // encodeURIComponent: ' ' -> '%20', '&' -> '%26', '=' -> '%3D', '+' -> '%2B'
    assert.equal(url, 'http://api.example.com/x?a%20b=x%26y%3Dz%2B1');
  });

  it('URL-encodes unicode in value', () => {
    const { url } = buildFetchInit(
      input({ params: [kv('q', '中文')] }),
    );
    // '中' -> %E4%B8%AD, '文' -> %E6%96%87
    assert.equal(url, 'http://api.example.com/x?q=%E4%B8%AD%E6%96%87');
  });
});

// ============================================================
// B. Header ordering / auto Content-Type / auto Accept
// ============================================================

describe('http-build: headers', () => {
  it('emits a user header alongside the auto-injected Accept', () => {
    const { init } = buildFetchInit(
      input({ headers: [kv('X-Trace', 'abc')] }),
    );
    assert.deepEqual(init.headers, {
      'X-Trace': 'abc',
      Accept: 'application/json',
    });
  });

  it('skips disabled headers', () => {
    const { init } = buildFetchInit(
      input({ headers: [kv('A', '1', false), kv('B', '2')] }),
    );
    assert.deepEqual(init.headers, { B: '2', Accept: 'application/json' });
  });

  it('skips headers with empty key', () => {
    const { init } = buildFetchInit(
      input({ headers: [kv('', 'orphan'), kv('B', '2')] }),
    );
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
    assert.equal((init.headers as Record<string, string>)['content-type'], 'application/vnd.custom+json');
    assert.equal((init.headers as Record<string, string>)['Content-Type'], undefined);
  });

  it('auto-adds Content-Type for json mode when user has none', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'json', content: '{}' } }),
    );
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/json');
  });

  it('auto-adds Content-Type for form mode', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'form', content: 'a=1' } }),
    );
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/x-www-form-urlencoded');
  });

  it('auto-adds Content-Type for raw mode', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'raw', content: 'hello' } }),
    );
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'text/plain');
  });

  it('does NOT auto-add Content-Type when body.mode is none', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'none', content: '' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], undefined);
  });

  it('auto-adds Content-Type even when json content is empty (mode implies a body shape)', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'json', content: '' } }),
    );
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/json');
  });

  it('auto-adds Accept: application/json when user has none', () => {
    const { init } = buildFetchInit(input());
    assert.equal((init.headers as Record<string, string>)['Accept'], 'application/json');
  });

  it('respects user-set Accept (case-insensitive)', () => {
    const { init } = buildFetchInit(
      input({ headers: [kv('accept', 'text/html')] }),
    );
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
      input({ auth: { type: 'bearer', bearer: 'eyJ.tok' } as AuthConfig }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer eyJ.tok');
  });

  it('resolves {{token}} in bearer against env', () => {
    const { init } = buildFetchInit(
      input({
        auth: { type: 'bearer', bearer: '{{token}}' } as AuthConfig,
        env: ENV,
      }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer eyJ.tok');
  });

  it('leaves missing var literal when env is absent', () => {
    const { init } = buildFetchInit(
      input({ auth: { type: 'bearer', bearer: '{{missing}}' } as AuthConfig }),
    );
    assert.equal((init.headers as Record<string, string>)['Authorization'], 'Bearer {{missing}}');
  });

  it('user Authorization header wins; auth is not appended twice', () => {
    const { init } = buildFetchInit(
      input({
        headers: [kv('Authorization', 'Bearer user-override')],
        auth: { type: 'bearer', bearer: 'eyJ.tok' } as AuthConfig,
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
          type: 'basic',
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
          type: 'basic',
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
          type: 'basic',
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
          type: 'basic',
          basic: { username: 'a', password: 'b' },
        } as AuthConfig,
      }),
    );
    assert.equal(
      (init.headers as Record<string, string>)['Authorization'],
      'Bearer override',
    );
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
          type: 'apikey',
          apikey: { key: 'X-API-Key', value: 'k_123', in_: 'header' },
        } as AuthConfig,
      }),
    );
    assert.equal((init.headers as Record<string, string>)['X-API-Key'], 'k_123');
  });

  it('in header mode: resolves {{apiKey}} from env', () => {
    const { init } = buildFetchInit(
      input({
        auth: {
          type: 'apikey',
          apikey: { key: 'X-API-Key', value: '{{apiKey}}', in_: 'header' },
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
          type: 'apikey',
          apikey: { key: 'X-API-Key', value: 'k_123', in_: 'header' },
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
          type: 'apikey',
          apikey: { key: 'apiKey', value: 'k_123', in_: 'query' },
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
          type: 'apikey',
          apikey: { key: 'apiKey', value: 'k_123', in_: 'query' },
        } as AuthConfig,
      }),
    );
    assert.equal(url, 'http://api.example.com/x?foo=bar&apiKey=k_123');
  });

  it('in query mode: URL-encodes key and value', () => {
    const { url } = buildFetchInit(
      input({
        auth: {
          type: 'apikey',
          apikey: { key: 'a key', value: 'x&y', in_: 'query' },
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
    const { init } = buildFetchInit(
      input({ headers: [kv('X-Region', '{{site}}')], env: ENV }),
    );
    assert.equal((init.headers as Record<string, string>)['X-Region'], 'cn');
  });

  it('leaves missing var literal (no env at all)', () => {
    const { url } = buildFetchInit(
      input({ url: 'http://api.example.com/{{missing}}/x' }),
    );
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
    const { init } = buildFetchInit(
      input({ body: { mode: 'json', content: '{"a":1}' } }),
    );
    assert.equal(init.body, '{"a":1}');
  });

  it('mode=form with content → body is the raw string', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'form', content: 'a=1&b=2' } }),
    );
    assert.equal(init.body, 'a=1&b=2');
  });

  it('mode=raw with content → body is the raw string', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'raw', content: 'hello world' } }),
    );
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
        type: 'apikey',
        apikey: { key: 'k', value: 'v', in_: 'query' },
      } as AuthConfig,
    });
    const fetch = buildFetchInit(i);
    const curl = buildCurlCommand(i);
    assert.equal(fetch.url, 'http://api.example.com/x?page=1&k=v');
    assert.ok(curl.includes("'http://api.example.com/x?page=1&k=v'"));
  });

  it('includes Content-Type, Accept, and Authorization headers', () => {
    const i = input({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1}' },
      auth: { type: 'bearer', bearer: 'eyJ.tok' } as AuthConfig,
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
      auth: { type: 'bearer', bearer: 'tok' } as AuthConfig,
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
        type: 'apikey',
        apikey: { key: 'k', value: 'v', in_: 'query' },
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
