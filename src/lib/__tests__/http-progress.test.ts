/**
 * 带进度的发送（`src/lib/http-progress.ts` 的 SSE 那一半）。
 *
 * 三件容易做错的事：进度帧要**逐帧**交出去（攒到最后等于没有进度条），终局信封必须是
 * `/api/http/send` 那一个形状（调用方拿它当普通发送的返回值），以及流在终局之前断掉要
 * 报错而不是返回半份结果。
 *
 * 在 Node 下跑：`IS_TAURI_SHELL` 在无 `window` 时为假，故走 `fetch` + `ReadableStream`。
 */
import { afterEach, describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { sendWithProgress, type HttpTransferProgress } from '../http-progress';

/** 一帧进度。 */
function progressFrame(sentBytes: number, totalBytes: number, server?: number): string {
  const progress: Record<string, number> = { sentBytes, totalBytes };
  if (server !== undefined) progress.serverReceivedBytes = server;
  return `data: ${JSON.stringify({ type: 'progress', progress })}\n`;
}

function streamOf(lines: string[]): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const line of lines) controller.enqueue(encoder.encode(line));
      controller.close();
    },
  });
  return new Response(body, { status: 200, headers: { 'content-type': 'text/event-stream' } });
}

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

/** 把后端的流换成给定的这份，顺手记下请求打到哪个地址。 */
function stubStream(lines: string[]): string[] {
  const calls: string[] = [];
  globalThis.fetch = (async (input: string | URL | Request) => {
    calls.push(
      typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url,
    );
    return streamOf(lines);
  }) as typeof fetch;
  return calls;
}

const OUTCOME = { http: { ok: true, response: { status: 200 } } };

describe('sendWithProgress（Web / SSE）', () => {
  it('逐帧交出进度，终局返回 done 里的信封', async () => {
    stubStream([
      progressFrame(0, 100),
      progressFrame(50, 100, 10),
      `data: ${JSON.stringify({ type: 'done', outcome: OUTCOME })}\n`,
    ]);

    const seen: HttpTransferProgress[] = [];
    const result = await sendWithProgress<typeof OUTCOME>({}, [], undefined, (p) => seen.push(p));

    assert.deepEqual(seen, [
      {
        sentBytes: 0,
        totalBytes: 100,
        serverReceivedBytes: undefined,
        serverTotalBytes: undefined,
      },
      { sentBytes: 50, totalBytes: 100, serverReceivedBytes: 10, serverTotalBytes: undefined },
    ]);
    assert.deepEqual(result, OUTCOME);
  });

  it('不带 data: 前缀的帧也认（代理会剥掉它）', async () => {
    stubStream([
      `${JSON.stringify({ type: 'progress', progress: { sentBytes: 1, totalBytes: 2 } })}\n`,
      `${JSON.stringify({ type: 'done', outcome: OUTCOME })}\n`,
    ]);
    const seen: number[] = [];
    await sendWithProgress({}, [], undefined, (p) => seen.push(p.sentBytes));
    assert.deepEqual(seen, [1]);
  });

  it('心跳与坏行被跳过，不影响后续的帧', async () => {
    stubStream([
      ': keep-alive\n',
      '\n',
      'not json\n',
      '{"type":"nonsense"}\n',
      progressFrame(7, 9),
      `data: ${JSON.stringify({ type: 'done', outcome: OUTCOME })}\n`,
    ]);
    const seen: number[] = [];
    await sendWithProgress({}, [], undefined, (p) => seen.push(p.sentBytes));
    assert.deepEqual(seen, [7]);
  });

  it('终局帧没有换行也送达', async () => {
    stubStream([
      progressFrame(1, 1),
      `data: ${JSON.stringify({ type: 'done', outcome: OUTCOME })}`,
    ]);
    assert.deepEqual(await sendWithProgress({}, [], undefined, () => {}), OUTCOME);
  });

  it('done 之前的 error 帧变成 reject，且带上原因', async () => {
    stubStream([
      progressFrame(10, 100),
      `data: ${JSON.stringify({ type: 'error', message: '上游拒绝连接' })}\n`,
    ]);
    await assert.rejects(
      sendWithProgress({}, [], undefined, () => {}),
      /上游拒绝连接/,
    );
  });

  it('流在终局之前结束 → reject', async () => {
    stubStream([progressFrame(10, 100)]);
    await assert.rejects(
      sendWithProgress({}, [], undefined, () => {}),
      /终局事件之前结束/,
    );
  });

  it('非 2xx 带上响应体里那句话', async () => {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ code: 'validation', message: '请求体超限' }), {
        status: 413,
        headers: { 'content-type': 'application/json' },
      })) as unknown as typeof fetch;
    await assert.rejects(
      sendWithProgress({}, [], undefined, () => {}),
      /请求体超限/,
    );
  });

  it('打到 send-progress 路由，而不是普通 send', async () => {
    const calls = stubStream([`data: ${JSON.stringify({ type: 'done', outcome: OUTCOME })}\n`]);
    await sendWithProgress({}, [], undefined, () => {});
    assert.deepEqual(calls, ['/api/http/send-progress']);
  });

  it('带文件时换成上传路线，请求体是 FormData', async () => {
    const seen: Array<{ url: string; init?: RequestInit }> = [];
    globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
      seen.push({
        url:
          typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url,
        init,
      });
      return streamOf([`data: ${JSON.stringify({ type: 'done', outcome: OUTCOME })}\n`]);
    }) as typeof fetch;

    const blob = new File(['AAAAA'], 'a.txt');
    const result = await sendWithProgress<typeof OUTCOME>(
      { method: 'POST', url: 'http://x/y' },
      [blob],
      undefined,
      () => {},
    );

    assert.deepEqual(result, OUTCOME);
    assert.deepEqual(
      seen.map((call) => call.url),
      ['/api/http/send-upload-progress'],
    );
    const body = seen[0]?.init?.body;
    assert.ok(body instanceof FormData, 'multipart 路线交出的是 FormData');
    const envelope = body.get('request');
    assert.ok(envelope instanceof Blob);
    assert.equal(await envelope.text(), JSON.stringify({ method: 'POST', url: 'http://x/y' }));
    assert.ok(body.get('a.txt') instanceof File);
  });
});
