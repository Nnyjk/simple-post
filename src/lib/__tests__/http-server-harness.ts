/**
 * `sendRequest` 集成测试共用的服务端脚手架：**不是**测试文件（不匹配
 * `*.test.ts`，故不被 `scripts/run-tests.mjs` 发现）。
 *
 * 每个测试文件是 node:test 的独立进程，所以本模块的模块级状态天然按文件隔离：
 * `startHarness()` 起自己的 `node:http` 服务并把桥接桩装到 `globalThis.fetch`，
 * `stopHarness()` 原样还原。
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import assert from 'node:assert/strict';
import type { AddressInfo } from 'node:net';

import { sendRequest, type SendInput } from '../http';
import type { RequestSettings } from '../../stores/app-store';
import type { Environment } from '../../types/domain';

export interface CapturedRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
}

export interface ScriptedResponse {
  status?: number;
  headers?: Record<string, string>;
  body?: string;
  /** 发送响应前的延迟（毫秒）。 */
  delayMs?: number;
}

export const captured: CapturedRequest[] = [];

const DEFAULT_RESPONSE: ScriptedResponse = {
  status: 200,
  headers: { 'content-type': 'application/json' },
  body: '{"ok":true}',
};

let nextResponse: ScriptedResponse = { ...DEFAULT_RESPONSE };
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
    // 后端的返回形状：传输层的信封 + 脚本的产出。harness 不跑脚本，故 script 缺席。
    const result = {
      http: {
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
        http: {
          ok: false,
          error: {
            kind: 'timeout',
            message: `Request timeout after ${sendBody.timeoutMs}ms`,
          },
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
      http: {
        ok: false,
        error: {
          kind: 'network',
          message: (err as Error)?.message ?? String(err),
        },
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

export function reply(res: ServerResponse, r: ScriptedResponse) {
  res.statusCode = r.status ?? 200;
  for (const [k, v] of Object.entries(r.headers ?? {})) res.setHeader(k, v);
  res.end(r.body ?? '');
}

export async function startHarness(): Promise<void> {
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
      nextResponse = { ...DEFAULT_RESPONSE };
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
}

export async function stopHarness(): Promise<void> {
  globalThis.fetch = originalFetch;
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

/** 脚本化下一次响应。服务端在回完一次后自动复位成默认值。 */
export function scriptNext(r: ScriptedResponse): void {
  nextResponse = r;
}

/** 测试服务端实例——重定向用例要临时替换它的 `request` 监听器。 */
export function getServer(): Server {
  return server;
}

export function getBaseUrl(): string {
  return baseUrl;
}

// ---------- 辅助 ----------

export const ENV: Environment = {
  id: 'env_test',
  projectId: 'p_test',
  name: 'test',
  baseUrls: {},
  moduleBaseUrls: {},
  variables: { token: 'eyJ.tok', userId: 'u_demo', apiKey: 'k_123' },
  disabledVariables: [],
  isActive: true,
};

export function settings(overrides: Partial<RequestSettings> = {}): RequestSettings {
  return {
    timeoutMs: 5000,
    followRedirects: true,
    maxResponseSizeKb: 5120,
    ...overrides,
  };
}

export function send(
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

export function lastRequest(): CapturedRequest {
  const r = captured[captured.length - 1];
  assert.ok(r, 'expected at least one captured request');
  return r!;
}
