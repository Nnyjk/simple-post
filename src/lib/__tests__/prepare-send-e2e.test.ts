/**
 * End-to-end tests for the path the Send button actually takes:
 * `prepareRequest` → `sendPreparedRequest` → a **real local HTTP server**.
 *
 * Run via `npm test` (which invokes `tsx --test`).
 *
 * Why this file exists
 * --------------------
 * The two neighbours each cover half of this and neither covers the seam:
 *
 *   - `http-build.test.ts` (1000+ lines) pins what `prepareRequest`
 *     *produces* — base-URL join, `{{var}}` resolution, query assembly,
 *     header ordering, body inclusion — but never sends anything, so it
 *     cannot catch a value that is assembled correctly and then lost on
 *     the way to the wire.
 *   - `http.test.ts` does run `fetch` against a real server, but it drives
 *     the legacy `sendRequest` path, which resolves templates and assembles
 *     the query **inside the backend**. Different code, different rules.
 *
 * `sendPreparedRequest` is the one the UI uses, and its whole reason to
 * exist is that the sender and the cURL preview share one construction
 * core. That makes the seam worth pinning against a server: the assertion
 * becomes "the server received X" rather than "the builder returned X",
 * which is the only form that survives a bug living between the two.
 *
 * Coverage matrix:
 *   B. Environment `{{var}}` substitution — in the base URL, in the path,
 *      in a header value, and in a query param value; plus the
 *      unresolved-marker behaviour that drives the UI's warning.
 *   C. Multi-value assembly — three params (one disabled), two custom
 *      headers, a JSON body, and the auto-injected `accept` /
 *      `content-type`.
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type IncomingMessage, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

import { sendPreparedRequest } from '../http';
import { prepareRequest, type PrepareRequestInput } from '../http-build';
import type { RequestSettings } from '../../stores/app-store';
import type { Environment, KeyValue } from '../../types/domain';

// ---------- server scaffolding ----------

interface Captured {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
}

const captured: Captured[] = [];

let server: Server;
let baseUrl: string;

const readBody = (req: IncomingMessage): Promise<string> =>
  new Promise((resolve) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
  });

before(async () => {
  server = createServer(async (req, res) => {
    captured.push({
      method: req.method ?? '',
      url: req.url ?? '',
      headers: { ...req.headers } as Record<string, string>,
      body: await readBody(req),
    });
    // Echo the request back, so the assertion can also be read off the
    // response the UI would render — the same shape the throwaway echo
    // server used during manual browser verification.
    const payload = JSON.stringify({ ok: true, received: captured[captured.length - 1] });
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(payload);
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  baseUrl = `http://127.0.0.1:${port}`;
});

after(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

// ---------- bridge stub ----------

/** Body shape of `POST /api/http/send` (camelCase, mirrors Rust `HttpSendBody`). */
interface SendBody {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: string | null;
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
}

const originalFetch: typeof fetch = globalThis.fetch.bind(globalThis);

/**
 * Test stub for the serve-web `/api/http/send` bridge, mirroring the axum
 * `http_send` handler: unwrap the envelope body, perform the real upstream
 * request, reply with the `HttpResult` envelope `http.ts` expects.
 *
 * Only the `ok: true` path is modelled — this file is about what reaches the
 * server, so timeout / abort / network-error mapping is `http.test.ts`'s
 * job and duplicating it would add maintenance without adding coverage.
 */
async function mockBridgeFetch(
  input: string | URL | Request,
  init?: RequestInit,
): Promise<Response> {
  const urlStr =
    typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;
  if (urlStr !== '/api/http/send') {
    return originalFetch(input as Request | string | URL, init);
  }

  const sendBody = JSON.parse(typeof init?.body === 'string' ? init.body : '{}') as SendBody;
  const upstreamInit: RequestInit = {
    method: sendBody.method,
    headers: Object.fromEntries(sendBody.headers),
    redirect: sendBody.followRedirects ? 'follow' : 'manual',
  };
  if (sendBody.body != null) upstreamInit.body = sendBody.body;

  const upstream = await originalFetch(sendBody.url, upstreamInit);
  const text = await upstream.text();

  return new Response(
    JSON.stringify({
      ok: true,
      response: {
        status: upstream.status,
        statusText: upstream.statusText,
        durationMs: 0,
        sizeBytes: new Blob([text]).size,
        headers: [...upstream.headers.entries()],
        body: text,
        contentType: upstream.headers.get('content-type') ?? '',
        method: sendBody.method,
        url: upstream.url,
        truncated: false,
      },
    }),
    { status: 200, headers: { 'content-type': 'application/json' } },
  );
}

globalThis.fetch = mockBridgeFetch as unknown as typeof fetch;

// ---------- fixtures ----------

const SETTINGS: RequestSettings = {
  timeoutMs: 5000,
  followRedirects: true,
  maxResponseSizeKb: 0,
};

const ENV: Environment = {
  id: 'env_e2e',
  projectId: 'p_e2e',
  name: 'e2e',
  baseUrls: {},
  variables: {
    site: 'cn',
    region: 'east',
    userId: 'u_42',
    token: 'eyJ.e2e',
  },
  isActive: true,
};

function kv(key: string, value: string, enabled = true): KeyValue {
  return { id: `k_${key}`, key, value, enabled };
}

function prep(overrides: Partial<PrepareRequestInput> = {}): PrepareRequestInput {
  return {
    method: 'GET',
    urlPath: '/x',
    // A stored endpoint always resolves against *some* base URL, and an
    // empty one yields a relative path — which is fine for the preview but
    // not something the send bridge can hand to a server. Default to the
    // test server so every case exercises a real absolute request.
    baseUrl,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    env: ENV,
    ...overrides,
  };
}

/** Assemble with the production core, then send that exact object. */
async function send(overrides: Partial<PrepareRequestInput> = {}) {
  const request = prepareRequest(prep(overrides));
  const response = await sendPreparedRequest({ request, settings: SETTINGS });
  assert.equal(response.status, 200, `send failed: ${response.statusText}`);
  const last = captured[captured.length - 1];
  assert.ok(last, 'server captured nothing');
  return { request, response, last };
}

/** Query string of the captured URL, as a lookup map. */
function queryMap(url: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const pair of (url.split('?')[1] ?? '').split('&')) {
    if (!pair) continue;
    const eq = pair.indexOf('=');
    const k = eq === -1 ? pair : pair.slice(0, eq);
    out[decodeURIComponent(k)] = eq === -1 ? '' : decodeURIComponent(pair.slice(eq + 1));
  }
  return out;
}

// ============================================================
// B. Environment {{var}} substitution
// ============================================================

describe('e2e: {{var}} reaches the server substituted', () => {
  it('resolves vars in the base URL, the path and a header', async () => {
    const { request, last } = await send({
      method: 'GET',
      baseUrl: `${baseUrl}/api/{{site}}`,
      urlPath: '/users/{{userId}}',
      headers: [kv('X-Token', '{{token}}'), kv('X-Region', '{{region}}-1')],
    });

    assert.equal(request.missingVars.length, 0, 'builder should resolve every var');

    // The base URL carried {{site}} and the path carried {{userId}} — both
    // must be gone by the time the request leaves the client.
    assert.equal(last.url, `/api/cn/users/u_42`);
    assert.doesNotMatch(last.url, /\{\{/);

    assert.equal(last.headers['x-token'], 'eyJ.e2e');
    assert.equal(last.headers['x-region'], 'east-1');
  });

  it('substitutes a var used as a query param value', async () => {
    const { request, last } = await send({
      urlPath: '/search',
      params: [kv('q', '{{userId}}'), kv('lang', '{{site}}')],
    });

    const q = queryMap(last.url);
    assert.equal(q.q, 'u_42', 'param values are templates like every other field');
    assert.equal(q.lang, 'cn');
    assert.equal(request.missingVars.length, 0);
  });

  it('reports a param variable that could not be resolved', async () => {
    // The server gets the marker literally, but the user is told — which
    // is the whole point of `missingVars`. Before this was fixed the
    // marker went out encoded and silent.
    const { request, last } = await send({
      urlPath: '/search',
      params: [kv('q', '{{gone}}')],
    });

    assert.deepEqual(request.missingVars, ['gone']);
    assert.equal(queryMap(last.url).q, '{{gone}}');
  });

  it('does not report a variable on a param that never ships', async () => {
    // Same rule as headers: a disabled param is not this request's
    // problem, and warning about it would be a false alarm.
    const { request } = await send({
      urlPath: '/search',
      params: [kv('q', '{{gone}}', false)],
    });

    assert.deepEqual(request.missingVars, []);
  });

  it('drops a param whose variable resolves to an empty string', async () => {
    const { last } = await send({
      urlPath: '/search',
      params: [kv('q', '{{blank}}'), kv('keep', '1')],
      env: { ...ENV, variables: { ...ENV.variables, blank: '' } },
    });

    const q = queryMap(last.url);
    assert.equal(q.q, undefined, 'a var resolving to "" behaves like an empty value');
    assert.equal(q.keep, '1');
  });

  it('leaves an unresolvable var literal and reports it in missingVars', async () => {
    // The documented contract: the body/URL is sent as-is with the marker
    // intact, and `missingVars` is what the UI turns into a warning. Silently
    // sending `{{nope}}` would be worse than useless — it would look like a
    // server-side value.
    const { request, last } = await send({
      urlPath: '/users/{{nope}}',
    });

    assert.deepEqual(request.missingVars, ['nope']);
    assert.equal(last.url, '/users/%7B%7Bnope%7D%7D');
  });

  it('resolves a var in the base URL even when the path has none', async () => {
    const { last } = await send({
      baseUrl: `${baseUrl}/{{site}}/{{region}}`,
      urlPath: '/ping',
    });

    assert.equal(last.url, '/cn/east/ping');
  });
});

// ============================================================
// C. Multi-value params / headers / body assembly
// ============================================================

describe('e2e: multi-value params, headers and body arrive intact', () => {
  it('sends every enabled param, drops disabled ones, and keeps value encoding', async () => {
    const { last } = await send({
      urlPath: '/search',
      params: [
        kv('page', '2'),
        kv('size', '50'),
        kv('draft', 'should-not-ship', false),
        kv('q', 'a b&c=d'),
      ],
    });

    const q = queryMap(last.url);
    assert.equal(q.page, '2');
    assert.equal(q.size, '50');
    assert.equal(q['draft'], undefined, 'a disabled param must not reach the wire');
    assert.equal(q.q, 'a b&c=d', 'a value with & and = must not split the query string');
    assert.equal(last.url.split('?')[0], '/search');
  });

  it('sends custom headers and still auto-injects accept', async () => {
    const { request, last } = await send({
      method: 'GET',
      headers: [kv('X-Client', 'simple-post'), kv('X-Trace', 'abc123')],
    });

    assert.equal(last.headers['x-client'], 'simple-post');
    assert.equal(last.headers['x-trace'], 'abc123');
    assert.equal(last.headers['accept'], 'application/json', 'accept is auto-injected');

    // Same object, so the preview and the send cannot disagree about the
    // header list either.
    assert.deepEqual(
      request.headers.map(([k]) => k.toLowerCase()).filter((k) => k.startsWith('x-')),
      ['x-client', 'x-trace'],
    );
  });

  it('sends a JSON body verbatim and sets content-type', async () => {
    const payload = JSON.stringify({ name: 'ada', tags: ['x', 'y'], nested: { n: 1 } });
    const { last } = await send({
      method: 'POST',
      urlPath: '/users',
      body: { mode: 'json', content: payload },
    });

    assert.equal(last.method, 'POST');
    assert.equal(last.body, payload, 'the body must go out byte-for-byte');
    assert.match(last.headers['content-type'] ?? '', /application\/json/);
    // The echo server replies with what it received — read the assertion off
    // the response the UI would render, not just the captured side.
    assert.deepEqual(JSON.parse(last.body), { name: 'ada', tags: ['x', 'y'], nested: { n: 1 } });
  });

  it('templates the body before it reaches the server', async () => {
    // The body used to ship verbatim, which meant the frontend disagreed
    // with Rust's `resolve_endpoint` and a `{{userId}}` went out as data.
    // The assertion is on what the *server* received, not on what the
    // builder returned.
    const { last } = await send({
      method: 'POST',
      urlPath: '/users',
      body: { mode: 'json', content: '{"id":"{{userId}}","site":"{{site}}"}' },
    });

    assert.equal(last.body, '{"id":"u_42","site":"cn"}');
    assert.deepEqual(JSON.parse(last.body), { id: 'u_42', site: 'cn' });
  });

  it('assembles params, headers and body together in one request', async () => {
    const { last } = await send({
      method: 'POST',
      baseUrl: `${baseUrl}/{{site}}`,
      urlPath: '/orders/{{userId}}',
      params: [kv('dry', 'false'), kv('page', '1')],
      headers: [kv('Authorization', 'Bearer {{token}}'), kv('X-Region', '{{region}}')],
      body: { mode: 'json', content: '{"item":"widget","qty":3}' },
    });

    assert.equal(last.method, 'POST');
    assert.equal(last.url.split('?')[0], '/cn/orders/u_42');
    assert.deepEqual(queryMap(last.url), { dry: 'false', page: '1' });
    assert.equal(last.headers['authorization'], 'Bearer eyJ.e2e');
    assert.equal(last.headers['x-region'], 'east');
    assert.equal(last.body, '{"item":"widget","qty":3}');
  });

  it('sends nothing for mode=none even when content is set', async () => {
    const { request, last } = await send({
      method: 'POST',
      urlPath: '/ping',
      body: { mode: 'none', content: 'this must not be sent' },
    });

    assert.equal(request.body, null);
    assert.equal(last.body, '');
  });
});
