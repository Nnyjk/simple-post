/**
 * Integration tests for `sendRequest` — runs `fetch` against a real
 * local HTTP server (built on `node:http`) so the wire path is
 * exercised end-to-end.
 *
 * Run via `npm test`.
 *
 * Coverage matrix:
 *   1. All 7 HTTP methods arrive at the server with the right verb.
 *   2. Query parameters arrive in the URL.
 *   3. User headers + auto Content-Type + auto Accept all reach the server.
 *   4. Body is sent verbatim for json / form / raw; nothing for mode=none.
 *   5. Auth (Bearer / Basic / API Key header / API Key query) reaches the server.
 *   6. {{var}} templates in URL / header / auth resolve against env.
 *   7. Response: 200 / 404 / 500 status passthrough; JSON → bodyJson set,
 *      non-JSON → bodyJson undefined.
 *   8. settings.timeoutMs fires → status 0, statusText 'Timeout'.
 *   9. Caller-initiated abort → status 0, statusText 'Aborted'.
 *  10. Network error (server unreachable) → status 0, statusText 'Network Error'.
 *  11. settings.maxResponseSizeKb caps body and appends a truncation marker.
 *  12. settings.followRedirects: true follows, false surfaces 3xx.
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import {
  createServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from 'node:http';
import type { AddressInfo } from 'node:net';

import { sendRequest, type SendInput } from './http';
import type { RequestSettings } from '../stores/app-store';
import type { AuthConfig, Environment, HttpMethod, RequestBody } from '../types/domain';

// ---------- server scaffolding ----------

interface CapturedRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
}

interface ScriptedResponse {
  status?: number;
  headers?: Record<string, string>;
  body?: string;
  /** Delay before sending the response, in milliseconds. */
  delayMs?: number;
}

const captured: CapturedRequest[] = [];
let nextResponse: ScriptedResponse = {
  status: 200,
  headers: { 'content-type': 'application/json' },
  body: '{"ok":true}',
};

let server: Server;
let baseUrl: string;

function reply(res: ServerResponse, r: ScriptedResponse) {
  res.statusCode = r.status ?? 200;
  for (const [k, v] of Object.entries(r.headers ?? {})) res.setHeader(k, v);
  res.end(r.body ?? '');
}

before(async () => {
  server = createServer((req: IncomingMessage, res: ServerResponse) => {
    let body = '';
    req.on('data', (chunk: Buffer) => {
      body += chunk.toString('utf-8');
    });
    req.on('end', () => {
      captured.push({
        method: req.method ?? 'GET',
        url: req.url ?? '',
        headers: req.headers as Record<string, string>,
        body,
      });
      const r = nextResponse;
      // Reset scripted response to a safe default; tests override
      // before each call when they need a specific shape.
      nextResponse = {
        status: 200,
        headers: { 'content-type': 'application/json' },
        body: '{"ok":true}',
      };
      const doReply = () => reply(res, r);
      if (r.delayMs && r.delayMs > 0) setTimeout(doReply, r.delayMs);
      else doReply();
    });
  });
  await new Promise<void>((resolve) =>
    server.listen(0, '127.0.0.1', () => resolve()),
  );
  const addr = server.address() as AddressInfo;
  baseUrl = `http://127.0.0.1:${addr.port}`;
});

after(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

// ---------- helpers ----------

const ENV: Environment = {
  id: 'env_test',
  projectId: 'p_test',
  name: 'test',
  baseUrls: {},
  variables: { token: 'eyJ.tok', userId: 'u_demo', apiKey: 'k_123' },
  isActive: true,
};

function settings(overrides: Partial<RequestSettings> = {}): RequestSettings {
  return {
    timeoutMs: 5000,
    followRedirects: true,
    maxResponseSizeKb: 5120,
    ...overrides,
  };
}

function send(overrides: Partial<SendInput> = {}): Promise<Awaited<ReturnType<typeof sendRequest>>> {
  return sendRequest({
    method: 'GET',
    url: `${baseUrl}/api/x`,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'none' },
    env: ENV,
    settings: settings(),
    ...overrides,
  });
}

function lastRequest(): CapturedRequest {
  const r = captured[captured.length - 1];
  assert.ok(r, 'expected at least one captured request');
  return r!;
}

// ============================================================
// 1. All 7 HTTP methods
// ============================================================

describe('sendRequest: HTTP methods', () => {
  const METHODS: HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

  for (const m of METHODS) {
    it(`${m} arrives at the server`, async () => {
      captured.length = 0;
      const resp = await send({ method: m });
      assert.equal(resp.status, 200);
      assert.equal(lastRequest().method, m);
    });
  }
});

// ============================================================
// 2. Query parameters
// ============================================================

describe('sendRequest: query params', () => {
  it('appends enabled params', async () => {
    captured.length = 0;
    await send({ params: [{ id: 'p1', key: 'page', value: '1', enabled: true }] });
    assert.equal(lastRequest().url, '/api/x?page=1');
  });

  it('joins multiple params with &', async () => {
    captured.length = 0;
    await send({
      params: [
        { id: 'p1', key: 'page', value: '1', enabled: true },
        { id: 'p2', key: 'size', value: '20', enabled: true },
      ],
    });
    assert.equal(lastRequest().url, '/api/x?page=1&size=20');
  });

  it('skips disabled params', async () => {
    captured.length = 0;
    await send({
      params: [
        { id: 'p1', key: 'a', value: '1', enabled: true },
        { id: 'p2', key: 'b', value: '2', enabled: false },
      ],
    });
    assert.equal(lastRequest().url, '/api/x?a=1');
  });
});

// ============================================================
// 3. Headers + auto Content-Type + auto Accept
// ============================================================

describe('sendRequest: headers', () => {
  it('user header reaches the server', async () => {
    captured.length = 0;
    await send({ headers: [{ id: 'h1', key: 'X-Trace', value: 'abc', enabled: true }] });
    assert.equal(lastRequest().headers['x-trace'], 'abc');
  });

  it('auto-injects Content-Type for json body', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1}' },
    });
    assert.equal(lastRequest().headers['content-type'], 'application/json');
  });

  it('auto-injects Content-Type for form body', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'form', content: 'a=1&b=2' },
    });
    assert.equal(lastRequest().headers['content-type'], 'application/x-www-form-urlencoded');
  });

  it('auto-injects Content-Type for raw body', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'raw', content: 'hello world' },
    });
    assert.equal(lastRequest().headers['content-type'], 'text/plain');
  });

  it('does NOT inject Content-Type when body.mode is none', async () => {
    captured.length = 0;
    await send({ method: 'GET' });
    assert.equal(lastRequest().headers['content-type'], undefined);
  });

  it('auto-injects Accept: application/json', async () => {
    captured.length = 0;
    await send({ method: 'GET' });
    assert.equal(lastRequest().headers['accept'], 'application/json');
  });
});

// ============================================================
// 4. Body content reaches the server
// ============================================================

describe('sendRequest: body', () => {
  it('JSON body arrives as the raw string', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1,"b":"x"}' },
    });
    assert.equal(lastRequest().body, '{"a":1,"b":"x"}');
  });

  it('form body arrives as the raw string', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'form', content: 'a=1&b=2' },
    });
    assert.equal(lastRequest().body, 'a=1&b=2');
  });

  it('raw body arrives as the raw string', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'raw', content: 'hello world' },
    });
    assert.equal(lastRequest().body, 'hello world');
  });

  it('mode=none sends no body even for POST', async () => {
    captured.length = 0;
    await send({ method: 'POST', body: { mode: 'none', content: 'should not be sent' } });
    assert.equal(lastRequest().body, '');
  });

  it('mode=json with empty content sends no body', async () => {
    captured.length = 0;
    await send({ method: 'POST', body: { mode: 'json', content: '' } });
    assert.equal(lastRequest().body, '');
  });
});

// ============================================================
// 5. Auth
// ============================================================

describe('sendRequest: auth', () => {
  it('Bearer: Authorization header reaches the server', async () => {
    captured.length = 0;
    await send({ auth: { type: 'bearer', bearer: 'eyJ.tok' } as AuthConfig });
    assert.equal(lastRequest().headers['authorization'], 'Bearer eyJ.tok');
  });

  it('Bearer: resolves {{token}} from env', async () => {
    captured.length = 0;
    await send({ auth: { type: 'bearer', bearer: '{{token}}' } as AuthConfig, env: ENV });
    assert.equal(lastRequest().headers['authorization'], 'Bearer eyJ.tok');
  });

  it('Basic: Authorization header is base64(user:pass)', async () => {
    captured.length = 0;
    await send({
      auth: { type: 'basic', basic: { username: 'alice', password: 'secret' } } as AuthConfig,
    });
    const hdr = lastRequest().headers['authorization'];
    assert.ok(hdr?.startsWith('Basic '));
    const decoded = Buffer.from(hdr!.slice('Basic '.length), 'base64').toString('utf-8');
    assert.equal(decoded, 'alice:secret');
  });

  it('API Key (header) reaches the server as <key>: <value>', async () => {
    captured.length = 0;
    await send({
      auth: {
        type: 'apikey',
        apikey: { key: 'X-API-Key', value: 'k_123', in_: 'header' },
      } as AuthConfig,
    });
    assert.equal(lastRequest().headers['x-api-key'], 'k_123');
  });

  it('API Key (query) is appended to the URL', async () => {
    captured.length = 0;
    await send({
      auth: {
        type: 'apikey',
        apikey: { key: 'apiKey', value: 'k_123', in_: 'query' },
      } as AuthConfig,
    });
    assert.equal(lastRequest().url, '/api/x?apiKey=k_123');
    assert.equal(lastRequest().headers['x-api-key'], undefined);
    assert.equal(lastRequest().headers['apikey'], undefined);
  });

  it('API Key (header) with {{apiKey}} resolves from env', async () => {
    captured.length = 0;
    await send({
      auth: {
        type: 'apikey',
        apikey: { key: 'X-API-Key', value: '{{apiKey}}', in_: 'header' },
      } as AuthConfig,
      env: ENV,
    });
    assert.equal(lastRequest().headers['x-api-key'], 'k_123');
  });
});

// ============================================================
// 6. {{var}} resolution in URL
// ============================================================

describe('sendRequest: URL template resolution', () => {
  it('resolves {{userId}} in URL against env', async () => {
    captured.length = 0;
    await send({ url: `${baseUrl}/api/users/{{userId}}`, env: ENV });
    assert.equal(lastRequest().url, '/api/users/u_demo');
  });
});

// ============================================================
// 7. Response handling
// ============================================================

describe('sendRequest: response handling', () => {
  it('200 with JSON body → status, bodyJson populated', async () => {
    nextResponse = {
      status: 200,
      headers: { 'content-type': 'application/json' },
      body: '{"code":0,"data":{"id":"x"}}',
    };
    const resp = await send();
    assert.equal(resp.status, 200);
    assert.deepEqual(resp.bodyJson, { code: 0, data: { id: 'x' } });
    assert.equal(resp.contentType, 'application/json');
  });

  it('200 with non-JSON body → bodyJson undefined', async () => {
    nextResponse = {
      status: 200,
      headers: { 'content-type': 'text/plain' },
      body: 'plain text',
    };
    const resp = await send();
    assert.equal(resp.status, 200);
    assert.equal(resp.body, 'plain text');
    assert.equal(resp.bodyJson, undefined);
  });

  it('404 response surfaces the status (no throw)', async () => {
    nextResponse = {
      status: 404,
      headers: { 'content-type': 'application/json' },
      body: '{"code":40400,"message":"not found"}',
    };
    const resp = await send();
    assert.equal(resp.status, 404);
    assert.deepEqual(resp.bodyJson, { code: 40400, message: 'not found' });
  });

  it('500 response surfaces the status', async () => {
    nextResponse = {
      status: 500,
      headers: { 'content-type': 'application/json' },
      body: '{"code":50000,"message":"server error"}',
    };
    const resp = await send();
    assert.equal(resp.status, 500);
  });

  it('captures response headers in the record', async () => {
    nextResponse = {
      status: 200,
      headers: {
        'content-type': 'application/json',
        'x-request-id': 'req_abc',
      },
      body: '{}',
    };
    const resp = await send();
    assert.equal(resp.headers['x-request-id'], 'req_abc');
  });
});

// ============================================================
// 8. Timeout (settings.timeoutMs)
// ============================================================

describe('sendRequest: timeout', () => {
  it('returns status 0 / Timeout when server delays past settings.timeoutMs', async () => {
    nextResponse = { status: 200, delayMs: 1000, body: '{"too":"late"}' };
    const resp = await send({ settings: settings({ timeoutMs: 200 }) });
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Timeout');
    assert.match(resp.error ?? '', /timeout after 200ms/);
  });

  it('completes when server responds within timeout', async () => {
    nextResponse = { status: 200, body: '{"ok":1}' };
    const resp = await send({ settings: settings({ timeoutMs: 1000 }) });
    assert.equal(resp.status, 200);
  });
});

// ============================================================
// 9. Caller-initiated abort
// ============================================================

describe('sendRequest: caller abort', () => {
  it('returns status 0 / Aborted when caller aborts before response', async () => {
    nextResponse = { status: 200, delayMs: 200, body: '{}' };
    const ctrl = new AbortController();
    const promise = send({ signal: ctrl.signal });
    setTimeout(() => ctrl.abort(), 50);
    const resp = await promise;
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Aborted');
    assert.equal(resp.error, 'aborted');
  });

  it('still completes when caller has not aborted by response time', async () => {
    nextResponse = { status: 200, body: '{"ok":1}' };
    const ctrl = new AbortController();
    const resp = await send({ signal: ctrl.signal });
    assert.equal(resp.status, 200);
    assert.ok(!ctrl.signal.aborted);
  });
});

// ============================================================
// 10. Network error
// ============================================================

describe('sendRequest: network error', () => {
  it('returns status 0 / Network Error when port is unreachable', async () => {
    // Port 1 is reserved + nothing is listening on it; the connection
    // should be refused promptly. We use a tighter timeout to keep
    // the test fast.
    const resp = await sendRequest({
      method: 'GET',
      url: 'http://127.0.0.1:1/api/x',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { type: 'none' },
      settings: { timeoutMs: 1500, followRedirects: true, maxResponseSizeKb: 5120 },
    });
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Network Error');
    assert.ok(resp.error, 'expected an error message');
  });
});

// ============================================================
// 11. Size cap / truncation
// ============================================================

describe('sendRequest: size cap', () => {
  it('truncates body when response exceeds settings.maxResponseSizeKb', async () => {
    const big = 'x'.repeat(10 * 1024); // 10 KB
    nextResponse = {
      status: 200,
      headers: { 'content-type': 'text/plain' },
      body: big,
    };
    const resp = await send({
      settings: settings({ maxResponseSizeKb: 1 }), // 1 KB cap
    });
    assert.ok(resp.body.length <= 1024 + 64, `body too large: ${resp.body.length}`);
    assert.match(resp.body, /\[truncated at 1 KB\]$/);
  });

  it('passes through when response is within cap', async () => {
    nextResponse = {
      status: 200,
      headers: { 'content-type': 'text/plain' },
      body: 'small body',
    };
    const resp = await send({
      settings: settings({ maxResponseSizeKb: 64 }),
    });
    assert.equal(resp.body, 'small body');
    assert.ok(!resp.body.includes('[truncated'));
  });
});

// ============================================================
// 12. Redirect behavior
// ============================================================

describe('sendRequest: redirect', () => {
  type ReqListener = (req: IncomingMessage, res: ServerResponse) => void;
  it('follows 302 when settings.followRedirects is true', async () => {
    // First server: 302 → /redirected. Second server: 200 + body.
    // node:test runs sequentially per file, so we can stack two
    // local servers by listening on another ephemeral port. To keep
    // this test self-contained we instead use a redirect to the same
    // server (which returns 200 by default after the first request).
    captured.length = 0;
    let firstCall = true;
    const originalListeners = server.listeners('request').slice() as ReqListener[];
    server.removeAllListeners('request');
    server.on('request', ((req: IncomingMessage, res: ServerResponse) => {
      if (firstCall) {
        firstCall = false;
        res.statusCode = 302;
        res.setHeader('Location', `${baseUrl}/redirected`);
        res.end();
        return;
      }
      let body = '';
      req.on('data', (c: Buffer) => (body += c.toString('utf-8')));
      req.on('end', () => {
        captured.push({
          method: req.method ?? 'GET',
          url: req.url ?? '',
          headers: req.headers as Record<string, string>,
          body,
        });
        reply(res, { status: 200, body: '{"followed":true}', headers: { 'content-type': 'application/json' } });
      });
    }) as ReqListener);

    const resp = await send({ settings: settings({ followRedirects: true }) });
    assert.equal(resp.status, 200);
    assert.deepEqual(resp.bodyJson, { followed: true });
    assert.ok(captured.some((c) => c.url === '/redirected'));

    // Restore the default handler for subsequent tests in this file.
    server.removeAllListeners('request');
    for (const l of originalListeners) server.on('request', l);
  });

  it('surfaces 3xx when settings.followRedirects is false', async () => {
    captured.length = 0;
    nextResponse = {
      status: 302,
      headers: { Location: 'http://elsewhere.example/' },
      body: '',
    };
    const resp = await send({ settings: settings({ followRedirects: false }) });
    assert.equal(resp.status, 302);
    assert.equal(resp.headers['location'], 'http://elsewhere.example/');
  });
});
