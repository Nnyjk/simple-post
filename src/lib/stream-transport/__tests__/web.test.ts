/**
 * Web 形态的流式读取（`src/lib/stream-transport/web.ts`）。
 *
 * 这一层只做三件容易做错的事：
 *
 *   1. 事件按**行**交付，而流可能在任何位置断开——最后半行（尤其是不补换行的 `end`）
 *      必须在 EOF 时补交，否则面板停在 streaming；
 *   2. 后端关流却没发收尾事件时，客户端要自己补一条；
 *   3. 非 200 的响应体里通常有一句人话（`{"code","message"}`），错误里要带上它。
 *
 * 在 Node 下用 `ReadableStream` + 假 `fetch` 跑：`readApiBase` 在没有 `window` 时返回 ''，
 * 故请求打到 `/api/http/stream`。
 */
import { afterEach, describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { openHttpStream } from '../web';
import type { HttpStreamArgs, SseEndEvent, StreamErrorEvent } from '../types';

const ARGS = { streamId: 'stream-1' } as HttpStreamArgs;

function streamOf(chunks: string[], status = 200): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
  return new Response(body, { status, headers: { 'content-type': 'text/event-stream' } });
}

function jsonOf(payload: unknown, status: number): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

function stubFetch(response: Response): void {
  globalThis.fetch = (async () => response) as typeof fetch;
}

describe('web 事件流读取', () => {
  it('EOF 时补交最后半行：不补换行的 end 照样送达', async () => {
    // 第二条事件被切在不断处，整个 `end` 的尾巴都留在缓冲区里。
    stubFetch(
      streamOf([
        '{"type":"chunk","text":"hi","seq":1,"bytes":2}\n{"type":"end","durationMs":',
        '9,"sizeBytes":2,"truncated":false,"reason":"eof"}',
      ]),
    );
    const chunks: string[] = [];
    let resolveEnd!: (event: SseEndEvent) => void;
    const ended = new Promise<SseEndEvent>((r) => {
      resolveEnd = r;
    });
    openHttpStream(ARGS, {
      onChunk: (event) => chunks.push(event.text),
      onEnd: (event) => resolveEnd(event),
    });
    const event = await ended;
    assert.equal(event.durationMs, 9);
    assert.equal(event.reason, 'eof');
    assert.deepEqual(chunks, ['hi']);
  });

  it('后端关流却没发 end：本地补一条 truncated 的收尾', async () => {
    stubFetch(streamOf(['{"type":"chunk","text":"hi","seq":1,"bytes":2}\n']));
    const end = await new Promise<SseEndEvent>((resolve) => {
      openHttpStream(ARGS, { onEnd: resolve });
    });
    assert.equal(end.reason, 'eof');
    assert.equal(end.truncated, true);
  });

  it('非 200：把响应体里的 message 拼进错误', async () => {
    stubFetch(jsonOf({ code: 500, message: 'upstream exploded' }, 500));
    const failed = await new Promise<StreamErrorEvent>((resolve) => {
      openHttpStream(ARGS, { onError: resolve });
    });
    assert.equal(failed.kind, 'network');
    assert.equal(failed.message, 'HTTP 500 /api/http/stream：upstream exploded');
  });

  it('非 200 且响应体不是 JSON：保持原样', async () => {
    stubFetch(new Response('<html>bad gateway</html>', { status: 502 }));
    const failed = await new Promise<StreamErrorEvent>((resolve) => {
      openHttpStream(ARGS, { onError: resolve });
    });
    assert.equal(failed.message, 'HTTP 502 /api/http/stream');
  });
});
