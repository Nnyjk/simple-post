/**
 * Web 形态的流式传输：后端把事件写成 `text/event-stream` 的 JSON 行，这里用 `fetch` +
 * `ReadableStream` 逐段读（`webInvokeCmd` 只收 JSON 响应，读不了流）。
 *
 * 取消与关闭的次序是「先 DELETE → 等后端把收尾事件送回来 → 超过宽限时间才中止本地读取」。
 * 中止排在前面会立刻结束本地读取，把后端随后的 `end{stopped}` / `closed` 一起丢掉，面板
 * 就永远停在运行中、SSE 也不写历史。（桌面形态的取消/关闭走另一条 IPC，Channel 不受影响。）
 *
 * bearer 与 base path 沿用 `readApiBase` / `readBearerFromStorage`，与其它 REST 调用同一套。
 */
import { readBearerFromStorage } from '../tauri-commands/bearer';
import { readApiBase } from '../tauri-commands';
import { dispatchSseEvent, dispatchWsEvent } from './dispatch';
import type {
  HttpStreamArgs,
  HttpStreamHandle,
  SseHandlers,
  StreamErrorEvent,
  WsConnectArgs,
  WsHandlers,
  WsSessionHandle,
} from './types';

function apiUrl(path: string): string {
  return `${readApiBase()}${path}`;
}

/** web 形态的请求头：注入的 bearer 加上本次调用自己的 header。 */
function webHeaders(extra: Record<string, string> = {}): Record<string, string> {
  const bearer = readBearerFromStorage();
  return { ...(bearer ? { authorization: `Bearer ${bearer}` } : {}), ...extra };
}

/** 停止一条 SSE 流之后，等后端把 `end` 送回来的上限。 */
const SSE_STOP_GRACE_MS = 5000;

/** 关闭一条 WS 会话之后，等 `closed` 到达的上限。 */
const WS_CLOSE_GRACE_MS = 3000;

/**
 * 收尾事件的兜底：后端会在 `end` / `closed` 之后关掉事件流，读循环随之结束，那就把定时器
 * 清掉；到点还没结束说明后端没有收尾，才真的中止本地读取。
 */
function abortAfterGrace(
  controller: AbortController,
  running: Promise<void>,
  graceMs: number,
): void {
  const timer = window.setTimeout(() => controller.abort(), graceMs);
  const clear = () => window.clearTimeout(timer);
  void running.then(clear, clear);
}

function asStreamError(error: unknown): StreamErrorEvent {
  return {
    type: 'error',
    kind: 'network',
    message: error instanceof Error ? error.message : String(error),
  };
}

/**
 * 读一条事件流：每行一个 JSON 事件（也接受 `data: ` 前缀的 SSE 帧）。切在行中间的那段留在
 * 缓冲区，下一段接着拼。
 */
async function readEventStream(response: Response, onEvent: (raw: unknown) => void): Promise<void> {
  if (response.body === null) throw new Error('流式响应没有 body');
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let newline = buffer.indexOf('\n');
    while (newline !== -1) {
      const rawLine = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      const line = rawLine.endsWith('\r') ? rawLine.slice(0, -1) : rawLine;
      if (line !== '') {
        const text = line.startsWith('data:') ? line.slice(5).trimStart() : line;
        try {
          onEvent(JSON.parse(text));
        } catch {
          // 不是 JSON 的行（心跳、代理插入的文本）没有事件语义，丢掉。
        }
      }
      newline = buffer.indexOf('\n');
    }
  }
}

/** 后台建连：失败只交给 `onError`，因为调用方拿到句柄时连接已经开始，没有 await 点可以拦。 */
async function connect(
  url: string,
  init: RequestInit,
  onEvent: (raw: unknown) => void,
  aborted: () => boolean,
  onError: (event: StreamErrorEvent) => void,
): Promise<void> {
  try {
    const response = await fetch(url, init);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status} ${url}`);
    }
    await readEventStream(response, onEvent);
  } catch (error) {
    if (aborted()) return;
    onError(asStreamError(error));
  }
}

export function openHttpStream(args: HttpStreamArgs, handlers: SseHandlers): HttpStreamHandle {
  const controller = new AbortController();
  const running = connect(
    apiUrl('/api/http/stream'),
    {
      method: 'POST',
      headers: webHeaders({ 'content-type': 'application/json', accept: 'text/event-stream' }),
      body: JSON.stringify(args),
      signal: controller.signal,
    },
    (raw) => dispatchSseEvent(raw, handlers),
    () => controller.signal.aborted,
    (event) => handlers.onError?.(event),
  );

  return {
    cancel(): void {
      // 先让后端停下：`end{reason:'stopped'}` 要靠这条流送回来，面板与响应历史都等着它。
      void fetch(apiUrl(`/api/http/stream/${encodeURIComponent(args.streamId)}`), {
        method: 'DELETE',
        headers: webHeaders(),
      }).catch(() => {});
      abortAfterGrace(controller, running, SSE_STOP_GRACE_MS);
    },
  };
}

export function openWsSession(args: WsConnectArgs, handlers: WsHandlers): WsSessionHandle {
  const controller = new AbortController();
  const sessionPath = `/api/ws/sessions/${encodeURIComponent(args.sessionId)}`;
  const running = connect(
    apiUrl('/api/ws/sessions'),
    {
      method: 'POST',
      headers: webHeaders({ 'content-type': 'application/json', accept: 'text/event-stream' }),
      body: JSON.stringify(args),
      signal: controller.signal,
    },
    (raw) => dispatchWsEvent(raw, handlers),
    () => controller.signal.aborted,
    (event) => handlers.onError?.(event),
  );

  return {
    async send(kind, data, base64): Promise<void> {
      const response = await fetch(apiUrl(`${sessionPath}/send`), {
        method: 'POST',
        headers: webHeaders({ 'content-type': 'application/json' }),
        body: JSON.stringify({ kind, data, base64: base64 ?? null }),
      });
      if (!response.ok) throw new Error(`WebSocket 发送失败：HTTP ${response.status}`);
    },
    async close(code, reason): Promise<void> {
      const params = new URLSearchParams();
      if (code !== undefined) params.set('code', String(code));
      if (reason) params.set('reason', reason);
      const query = params.size === 0 ? '' : `?${params.toString()}`;
      // 关闭失败不弹到界面：服务端可能已经把会话摘掉了，DELETE 回 404 是正常的。调用方是
      // `void closeWs(...)`，这里漏出去就是一条未处理的 rejection。
      try {
        await fetch(apiUrl(`${sessionPath}${query}`), {
          method: 'DELETE',
          headers: webHeaders(),
        });
      } catch {
        // 网络层失败同样只意味着「这条会话没机会通知后端」，本地仍要按已断开处理。
      }
      // `closed` 事件随后从这条流回来，面板靠它把状态落到「未连接」。
      abortAfterGrace(controller, running, WS_CLOSE_GRACE_MS);
    },
  };
}
