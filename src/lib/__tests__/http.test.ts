/**
 * `sendRequest` 的集成测试 —— 对一个真实的本地 HTTP 服务（基于 `node:http`）跑
 * `fetch`，把线上路径端到端走一遍。通过 `npm test` 运行。
 *
 * 覆盖矩阵：
 *   1. 全部 7 个 HTTP 方法以正确的动词到达服务端。
 *   2. 查询参数出现在 URL 里。
 *   3. 用户 header + 自动 Content-Type + 自动 Accept 都到达服务端。
 *   4. body 对 json / form / raw 逐字送达；mode=none 则不送。
 *   5. 认证（Bearer / Basic / API Key header / API Key query）到达服务端。
 *   6. URL / header / 认证里的 {{var}} 模板对环境解析。
 *   7. 响应：200 / 404 / 500 状态透传；JSON → 设置 bodyJson，非 JSON → bodyJson 未定义。
 *   8. settings.timeoutMs 触发 → status 0、statusText 'Timeout'。
 *   9. 调用方发起中止 → status 0、statusText 'Aborted'。
 *  10. 网络错误（服务端不可达）→ status 0、statusText 'Network Error'。
 *  11. settings.maxResponseSizeKb 限制体积并追加截断标记。
 *  12. settings.followRedirects：true 跟随，false 把 3xx 暴露出来。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

import { sendRequest, type SendInput } from '../http';
import type { RequestSettings } from '../../stores/app-store';
import type { AuthConfig, Environment, HttpMethod } from '../../types/domain';

// ---------- 服务端脚手架 ----------

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
  /** 发送响应前的延迟（毫秒）。 */
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

/** `POST /api/http/send` 的 body 形状（camelCase，镜像 Rust 的
 *  `HttpSendBody`）。放在这里，好让 fetch mock 能解析 http.ts 发出的东西。 */
interface SendBody {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: string | null;
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
}

/** 原始的 `globalThis.fetch` —— mock 用它在剥掉桥接信封后，把真实 HTTP 请求
 *  转发给测试服务端。 */
const originalFetch: typeof fetch = globalThis.fetch.bind(globalThis);

/**
 * serve-web `/api/http/send` 桥接的测试桩。
 *
 * 生产环境里这个桥接由真正的 axum `http_send` handler 执行
 * （`src-tauri/src/web_api/routes/nodes.rs`）：它反序列化 `HttpSendBody`、跑
 * `http_executor::execute`、返回 `HttpResult` 信封。要在 Node 测试里走通
 * http.ts → invokeCmd → webFetch 这条路，就得拦下 `globalThis.fetch`、认出相对的
 * `/api/http/send` URL，然后围绕真实上游服务端的返回内容回一个等价信封。
 *
 * 镜像生产语义：
 *   - timeoutMs → 到点触发的 AbortController；返回的信封是
 *     `ok:false, error:{kind:'timeout'}`，与 Rust 在 reqwest 超时时返回的形状相同。
 *   - followRedirects=false → 原生 fetch 的 `redirect:'manual'`，在捕获的请求里保留
 *     3xx 的 body。
 *   - 调用方 AbortSignal → 传给上游 fetch；触发时我们重新抛出，好让 http.ts 把它
 *     映射成 `abortedResponse`。
 *   - 传输失败（ECONNREFUSED、DNS 等）→ `ok:false, error:{kind:'network'}` 的信封，
 *     与 Rust 在 `reqwest::Error` 时返回的相同。
 */
async function mockBridgeFetch(
  input: string | URL | Request,
  init?: RequestInit,
): Promise<Response> {
  const urlStr =
    typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;
  if (urlStr !== '/api/http/send') {
    // 其余情况（本套件里不该发生）——放行。
    return originalFetch(input as Request | string | URL, init);
  }

  const rawBody = typeof init?.body === 'string' ? init.body : '{}';
  const sendBody = JSON.parse(rawBody) as SendBody;

  // 把超时与调用方信号合成一个控制器，任一来源都能中止上游 fetch。
  const ctrl = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    ctrl.abort();
  }, sendBody.timeoutMs);
  const callerSignal = init?.signal;
  if (callerSignal) {
    if (callerSignal.aborted) {
      clearTimeout(timer);
      throw callerSignal.reason ?? new DOMException('aborted', 'AbortError');
    }
    const onCallerAbort = () => ctrl.abort();
    callerSignal.addEventListener('abort', onCallerAbort, { once: true });
  }

  const upstreamInit: RequestInit = {
    method: sendBody.method,
    headers: Object.fromEntries(sendBody.headers),
    redirect: sendBody.followRedirects ? 'follow' : 'manual',
    signal: ctrl.signal,
  };
  if (sendBody.body != null) upstreamInit.body = sendBody.body;

  try {
    const upstream = await originalFetch(sendBody.url, upstreamInit);
    const rawText = await upstream.text();
    // 镜像 Rust 的 `http_executor::truncate_body`：按 UTF-8 字符边界截断字节，并
    // 追加生产服务端返回的那个标记。默认上限 0 = 不截断（对应 Rust 的
    // `if max_bytes == 0 { return (body, false); }` 分支）。
    const maxBytes = sendBody.maxResponseSizeKb * 1024;
    let text = rawText;
    let truncated = false;
    if (maxBytes > 0 && rawText.length > maxBytes) {
      let cut = maxBytes;
      // 往回退到一个 UTF-8 首字节（或 ASCII 字节），好让切片不劈开多字节码点。镜像
      // Rust 里的 `while cut > 0 && !body.is_char_boundary(cut)` 循环，又不依赖
      // `String.prototype.isCharBoundary`（Node 里没有）。
      while (cut > 0) {
        const code = rawText.charCodeAt(cut - 1);
        if (code < 0x80 || code >= 0xc0) break;
        cut -= 1;
      }
      text = `${rawText.slice(0, cut)}\n\n[truncated at ${maxBytes} bytes by Tauri HTTP layer]`;
      truncated = true;
    }
    const result = {
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
        truncated,
      },
    };
    return new Response(JSON.stringify(result), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  } catch (err) {
    if (timedOut) {
      // Rust 在 reqwest 超时时返回 ok:false / error.kind=timeout。
      const result = {
        ok: false,
        error: {
          kind: 'timeout',
          message: `Request timeout after ${sendBody.timeoutMs}ms`,
        },
      };
      return new Response(JSON.stringify(result), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }
    const name = (err as { name?: string })?.name;
    if (name === 'AbortError' || /abort/i.test((err as Error).message ?? '')) {
      // 调用方发起的中止 —— 重新抛出，好让 http.ts 去映射。
      throw err;
    }
    // 传输失败 —— kind=network 的信封（镜像 Rust）。
    const result = {
      ok: false,
      error: {
        kind: 'network',
        message: (err as Error)?.message ?? String(err),
      },
    };
    return new Response(JSON.stringify(result), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  } finally {
    clearTimeout(timer);
    if (callerSignal) {
      callerSignal.removeEventListener('abort', () => ctrl.abort());
    }
  }
}

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
      // 把脚本化响应重置成安全默认值；需要特定形状的测试会在每次调用前覆盖它。
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
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
  const addr = server.address() as AddressInfo;
  baseUrl = `http://127.0.0.1:${addr.port}`;
  // 桥接桩必须在服务端 listen 之后安装，好让桩里的上游 fetcher 有真实端口可拨。
  globalThis.fetch = mockBridgeFetch as unknown as typeof fetch;
});

after(async () => {
  globalThis.fetch = originalFetch;
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

// ---------- 辅助 ----------

const ENV: Environment = {
  id: 'env_test',
  projectId: 'p_test',
  name: 'test',
  baseUrls: {},
  moduleBaseUrls: {},
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

function send(
  overrides: Partial<SendInput> = {},
): Promise<Awaited<ReturnType<typeof sendRequest>>> {
  return sendRequest({
    method: 'GET',
    url: `${baseUrl}/api/x`,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
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
// 1. 全部 7 个 HTTP 方法
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
// 2. 查询参数
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
// 3. Header + 自动 Content-Type + 自动 Accept
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
// 4. body 内容到达服务端
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
// 5. 认证
// ============================================================

describe('sendRequest: auth', () => {
  it('Bearer: Authorization header reaches the server', async () => {
    captured.length = 0;
    await send({ auth: { kind: 'bearer', bearer: 'eyJ.tok' } as AuthConfig });
    assert.equal(lastRequest().headers['authorization'], 'Bearer eyJ.tok');
  });

  it('Bearer: resolves {{token}} from env', async () => {
    captured.length = 0;
    await send({ auth: { kind: 'bearer', bearer: '{{token}}' } as AuthConfig, env: ENV });
    assert.equal(lastRequest().headers['authorization'], 'Bearer eyJ.tok');
  });

  it('Basic: Authorization header is base64(user:pass)', async () => {
    captured.length = 0;
    await send({
      auth: { kind: 'basic', basic: { username: 'alice', password: 'secret' } } as AuthConfig,
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
        kind: 'apikey',
        apikey: { key: 'X-API-Key', value: 'k_123', inLocation: 'header' },
      } as AuthConfig,
    });
    assert.equal(lastRequest().headers['x-api-key'], 'k_123');
  });

  it('API Key (query) is appended to the URL', async () => {
    captured.length = 0;
    await send({
      auth: {
        kind: 'apikey',
        apikey: { key: 'apiKey', value: 'k_123', inLocation: 'query' },
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
        kind: 'apikey',
        apikey: { key: 'X-API-Key', value: '{{apiKey}}', inLocation: 'header' },
      } as AuthConfig,
      env: ENV,
    });
    assert.equal(lastRequest().headers['x-api-key'], 'k_123');
  });
});

// ============================================================
// 6. URL 里的 {{var}} 解析
// ============================================================

describe('sendRequest: URL template resolution', () => {
  it('resolves {{userId}} in URL against env', async () => {
    captured.length = 0;
    await send({ url: `${baseUrl}/api/users/{{userId}}`, env: ENV });
    assert.equal(lastRequest().url, '/api/users/u_demo');
  });
});

// ============================================================
// 7. 响应处理
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
// 8. 超时（settings.timeoutMs）
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
// 9. 调用方发起的中止
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
// 10. 网络错误
// ============================================================

describe('sendRequest: network error', () => {
  it('returns status 0 / Network Error when port is unreachable', async () => {
    // 端口 1 是保留端口且上面没有监听，连接应当很快被拒。这里用更紧的超时让测试保持快速。
    const resp = await sendRequest({
      method: 'GET',
      url: 'http://127.0.0.1:1/api/x',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      settings: { timeoutMs: 1500, followRedirects: true, maxResponseSizeKb: 5120 },
    });
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Network Error');
    assert.ok(resp.error, 'expected an error message');
  });
});

// ============================================================
// 11. 体积上限 / 截断
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
    // 截断由服务端的 `http_executor::truncate_body` 执行（两种形态调的是同一个 Rust
    // 函数），所以标记文本是 Rust 那边那个，而不是客户端旧的 `[truncated at N KB]`。
    assert.match(resp.body, /\[truncated at 1024 bytes by Tauri HTTP layer\]$/);
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
// 12. 重定向行为
// ============================================================

describe('sendRequest: redirect', () => {
  type ReqListener = (req: IncomingMessage, res: ServerResponse) => void;
  it('follows 302 when settings.followRedirects is true', async () => {
    // 第一个服务端：302 → /redirected。第二个服务端：200 + body。
    // node:test 在一个文件内顺序执行，所以可以在另一个临时端口上再叠一个本地
    // 服务端。为了让本测试自包含，这里改用重定向到同一个服务端（第一次请求之后它
    // 默认返回 200）。
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
        reply(res, {
          status: 200,
          body: '{"followed":true}',
          headers: { 'content-type': 'application/json' },
        });
      });
    }) as ReqListener);

    const resp = await send({ settings: settings({ followRedirects: true }) });
    assert.equal(resp.status, 200);
    assert.deepEqual(resp.bodyJson, { followed: true });
    assert.ok(captured.some((c) => c.url === '/redirected'));

    // 把默认 handler 恢复回去，供本文件后续测试使用。
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
