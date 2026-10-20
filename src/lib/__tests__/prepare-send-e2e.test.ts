/**
 * 「发送」按钮实际走的那条路径的端到端测试：
 * `prepareRequest` → `sendPreparedRequest` → **真实本地 HTTP 服务**。
 * 通过 `npm test` 运行（它调用 `tsx --test`）。
 *
 * 为什么要有这个文件
 * ------------------
 * 旁边两个文件各覆盖一半，都不覆盖这条接缝：
 *
 *   - `http-build.test.ts` 钉住 `prepareRequest` *产出*什么——base URL join、
 *     `{{var}}` 解析、查询串拼装、header 排序、body 是否包含——但从不真的发出去，
 *     所以抓不到「值组装对了却在去往线上的路上丢掉」这种情况。
 *   - `http.test.ts` 确实对真实服务端跑 `fetch`，但它驱动的是旧的 `sendRequest`
 *     路径，那条路径把模板解析与查询串组装放在**后端**里。代码不同，规则也不同。
 *
 * `sendPreparedRequest` 才是 UI 用的那个，而它存在的全部理由就是发送端与 cURL 预览
 * 共用一个构造核。所以这条接缝值得对着一个服务端来钉：断言变成「服务端收到了 X」而
 * 不是「构造器返回了 X」——只有后者能挡住活在两者之间的 bug。
 *
 * 覆盖矩阵：
 *   B. 环境 `{{var}}` 替换 —— 在 base URL 里、在 path 里、在 header 值里、在查询参数
 *      值里；外加驱动 UI 告警的未解析标记行为。
 *   C. 多值组装 —— 三个参数（其中一个禁用）、两个自定义 header、一个 JSON body，
 *      以及自动注入的 `accept` / `content-type`。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type IncomingMessage, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

import { sendPreparedRequest } from '../http';
import { prepareRequest, type PrepareRequestInput } from '../http-build';
import type { RequestSettings } from '../../stores/app-store';
import type { Environment, KeyValue } from '../../types/domain';

// ---------- 服务端脚手架 ----------

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
    // 把请求原样回显，好让断言既能读捕获到的内容，也能读 UI 会渲染的那个响应——
    // 与手工浏览器验证时用的临时 echo 服务是同一个形状。
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

// ---------- 桥接桩 ----------

/** `POST /api/http/send` 的 body 形状（camelCase，镜像 Rust 的 `HttpSendBody`）。 */
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
 * serve-web `/api/http/send` 桥接的测试桩，镜像 axum 的 `http_send` handler：
 * 剥掉信封 body、发真实上游请求、回 `http.ts` 期待的 `HttpResult` 信封。
 *
 * 只建模 `ok: true` 这条路——本文件讲的是到达服务端的东西，所以超时 / 中止 /
 * 网络错误的映射是 `http.test.ts` 的活，抄一遍只增加维护量不增加覆盖。
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

// ---------- 夹具 ----------

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
  moduleBaseUrls: {},
  variables: {
    site: 'cn',
    region: 'east',
    userId: 'u_42',
    token: 'eyJ.e2e',
  },
  disabledVariables: [],
  isActive: true,
};

function kv(key: string, value: string, enabled = true): KeyValue {
  return { id: `k_${key}`, key, value, enabled };
}

function prep(overrides: Partial<PrepareRequestInput> = {}): PrepareRequestInput {
  return {
    method: 'GET',
    urlPath: '/x',
    // 已存的 endpoint 总能解析出*某个* base URL，而空 base 会产出相对路径——对预览没
    // 问题，但不是发送桥能交给服务端的东西。默认给测试服务端，好让每个用例都在跑
    // 一个真实的绝对地址请求。
    baseUrl,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    env: ENV,
    ...overrides,
  };
}

/** 用生产构造核组装，然后发送那个确切的对象。 */
async function send(overrides: Partial<PrepareRequestInput> = {}) {
  const request = prepareRequest(prep(overrides));
  const response = await sendPreparedRequest({ request, settings: SETTINGS });
  assert.equal(response.status, 200, `send failed: ${response.statusText}`);
  const last = captured[captured.length - 1];
  assert.ok(last, 'server captured nothing');
  return { request, response, last };
}

/** 捕获到的 URL 的查询串，转成查找表。 */
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
// B. 环境 {{var}} 替换
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

    // base URL 带 `{{site}}`、path 带 `{{userId}}`——两者都必须在请求离开客户端前消失。
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
    // 服务端拿到的是字面标记，但用户被告知了——这正是 `missingVars` 的全部意义。
    const { request, last } = await send({
      urlPath: '/search',
      params: [kv('q', '{{gone}}')],
    });

    assert.deepEqual(request.missingVars, ['gone']);
    assert.equal(queryMap(last.url).q, '{{gone}}');
  });

  it('does not report a variable on a param that never ships', async () => {
    // 与 header 同规则：未启用的参数不算这次请求的问题，报它就是误报。
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
    // 有文档在案的契约：body/URL 原样带标记发出，而 `missingVars` 正是 UI 拿来变成
    // 告警的东西。悄悄发出 `{{nope}}` 比没用更糟——它看起来像个服务端变量。
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
// C. 多值的参数 / header / body 组装
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

    // 同一个对象，所以预览与发送在 header 列表上也不可能不一致。
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
    // echo 服务端回的就是它收到的内容——断言既读捕获侧，也读 UI 会渲染的那个响应。
    assert.deepEqual(JSON.parse(last.body), { name: 'ada', tags: ['x', 'y'], nested: { n: 1 } });
  });

  it('templates the body before it reaches the server', async () => {
    // body 在到达服务端之前就要被模板化，否则前端会与 Rust 的 `resolve_endpoint`
    // 不一致，`{{userId}}` 会作为数据发出去。断言落在*服务端*收到什么，而不是构造器
    // 返回什么。
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
