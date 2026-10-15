/**
 * `tauri-commands.test.ts` 各分片共用的 fetch 桩。**不是**测试文件（不匹配
 * `*.test.ts`，故不被 `scripts/run-tests.mjs` 发现）。
 *
 * 每个测试文件是 node:test 的独立进程，故本模块的 `calls` 天然按文件隔离。
 */
export interface CapturedCall {
  url: string;
  /** String for easier assertions; `undefined` when the request had no body. */
  body: string | undefined;
  method: string;
  contentType: string | undefined;
  signal: AbortSignal | undefined;
}

export let calls: CapturedCall[] = [];

/** 清空已捕获的调用。导出绑定不可直接赋值，故走这个函数。 */
export function resetCalls(): void {
  calls = [];
}

const originalFetch = globalThis.fetch;

export function installFetchMock(respondWith: { json?: unknown; status?: number } = {}): void {
  resetCalls();
  const status = respondWith.status ?? 200;
  const json = respondWith.json;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = typeof input === 'string' ? input : input.toString();
    let bodyStr: string | undefined;
    if (typeof init?.body === 'string') bodyStr = init.body;
    const contentType = init?.headers
      ? (new Headers(init.headers).get('content-type') ?? undefined)
      : undefined;
    calls.push({
      url,
      body: bodyStr,
      method: init?.method ?? 'GET',
      contentType,
      signal: init?.signal ?? undefined,
    });
    const text = json === undefined ? '' : JSON.stringify(json);
    return new Response(text, {
      status,
      headers: { 'content-type': 'application/json' },
    });
  }) as typeof fetch;
}

export function restoreFetchMock(): void {
  globalThis.fetch = originalFetch;
  resetCalls();
}
