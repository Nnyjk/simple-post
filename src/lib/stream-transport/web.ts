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
import { UPLOAD_ROUTES } from '../tauri-commands/web-routes';
import { uploadFiles, uploadForm } from '../web-uploads';
import { dispatchSseEvent, dispatchWsEvent } from './dispatch';
import type {
  HttpStreamArgs,
  HttpStreamHandle,
  SseEndEvent,
  SseHandlers,
  StreamErrorEvent,
  WsClosedEvent,
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
 * 收尾事件的回退：后端会在 `end` / `closed` 之后关掉事件流，读循环随之结束，那就把定时器
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
async function readEventStream(
  response: Response,
  onEvent: (raw: unknown) => void,
  isTerminal: (raw: unknown) => boolean,
): Promise<boolean> {
  if (response.body === null) throw new Error('流式响应没有 body');
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let sawTerminal = false;

  /** 交付一整行（已去掉行尾的 `\r`）；不是 JSON 的行没有事件语义，丢掉。 */
  const emitLine = (rawLine: string): void => {
    if (rawLine === '') return;
    const line = rawLine.endsWith('\r') ? rawLine.slice(0, -1) : rawLine;
    if (line === '') return;
    const text = line.startsWith('data:') ? line.slice(5).trimStart() : line;
    let event: unknown;
    try {
      event = JSON.parse(text);
    } catch {
      // 心跳、代理插入的文本。
      return;
    }
    if (isTerminal(event)) sawTerminal = true;
    onEvent(event);
  };

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let newline = buffer.indexOf('\n');
    while (newline !== -1) {
      emitLine(buffer.slice(0, newline));
      buffer = buffer.slice(newline + 1);
      newline = buffer.indexOf('\n');
    }
  }

  // EOF 时缓冲区里的残留不是垃圾：后端可能把最后一条事件（尤其是 `end`）写在流尾而不补
  // 换行，直接丢掉，面板就会永远停在 streaming。`decoder.decode()` 无参调用冲掉多字节
  // 字符的残片后，把剩下那半行当作一次正常的行交付。
  buffer += decoder.decode();
  emitLine(buffer);
  return sawTerminal;
}

/** 非 200 的响应体形如 `{"code","message"}`：把那句话拼进错误里；拿不到就只报状态与地址。 */
async function httpFailure(response: Response, url: string): Promise<Error> {
  const base = `HTTP ${response.status} ${url}`;
  try {
    const text = await response.text();
    const parsed: unknown = JSON.parse(text);
    if (typeof parsed === 'object' && parsed !== null) {
      const message = (parsed as { message?: unknown }).message;
      if (typeof message === 'string' && message.trim() !== '') {
        return new Error(`${base}：${message.trim()}`);
      }
    }
  } catch {
    // 不是 JSON（代理页、纯文本）——保持原文。
  }
  return new Error(base);
}

/** 后台建连：失败只交给 `onError`，因为调用方拿到句柄时连接已经开始，没有 await 点可以拦。 */
async function connect(
  url: string,
  init: RequestInit,
  onEvent: (raw: unknown) => void,
  aborted: () => boolean,
  onError: (event: StreamErrorEvent) => void,
  /** 什么算收尾事件（SSE 的 `end`、WS 的 `closed`，以及两种的 `error`）。 */
  isTerminal: (raw: unknown) => boolean,
  /**
   * 没收到收尾事件时补的那一条。后端把事件流关掉却没发 `end` / `closed`（或本地中止了
   * 读取）时，面板与响应历史都等这一条才会落地。
   */
  terminalFallback: () => unknown,
): Promise<void> {
  let terminalSeen = false;
  const emit = (raw: unknown): void => {
    if (isTerminal(raw)) terminalSeen = true;
    onEvent(raw);
  };
  try {
    const response = await fetch(url, init);
    if (!response.ok) throw await httpFailure(response, url);
    terminalSeen = await readEventStream(response, emit, isTerminal);
  } catch (error) {
    if (aborted()) {
      // 宽限时间内后端没把收尾事件送回来，本地读取被中止：仍然补一条，否则面板停在
      // 「运行中」、SSE 也不写历史。
      if (!terminalSeen) emit(terminalFallback());
      return;
    }
    onError(asStreamError(error));
    return;
  }
  if (!terminalSeen) emit(terminalFallback());
}

/** SSE 的收尾：`end` 正常结束，`error` 之后后端不再发事件。 */
function isTerminalSseEvent(raw: unknown): boolean {
  if (typeof raw !== 'object' || raw === null) return false;
  const { type } = raw as { type?: unknown };
  return type === 'end' || type === 'error';
}

function isTerminalWsEvent(raw: unknown): boolean {
  if (typeof raw !== 'object' || raw === null) return false;
  const { type } = raw as { type?: unknown };
  return type === 'closed' || type === 'error';
}

/**
 * 没等到 `end` 时的收尾。`durationMs` / `sizeBytes` 不知道就报 0，`truncated` 说真话：
 * 这条流没有按契约结束。`reason` 区分「后端关流没发 end」与「本地中止了读取」。
 */
function sseEndFallback(controller: AbortController): () => SseEndEvent {
  return () => ({
    type: 'end',
    durationMs: 0,
    sizeBytes: 0,
    truncated: true,
    reason: controller.signal.aborted ? 'stopped' : 'eof',
  });
}

function wsClosedFallback(): WsClosedEvent {
  return { type: 'closed', code: null, reason: '', at: Date.now() };
}

export function openHttpStream(args: HttpStreamArgs, handlers: SseHandlers): HttpStreamHandle {
  const controller = new AbortController();
  // 浏览器选中的文件没有服务端可读的路径：字节随这次请求一起上来，故换成 multipart 路线。
  // 那几条路由的响应同样是事件流，`connect` 之后的一切不变。
  let files: File[];
  try {
    files = uploadFiles(args.bodyMode, args.body);
  } catch (error) {
    handlers.onError?.(asStreamError(error));
    return { cancel(): void {} };
  }
  const running = connect(
    apiUrl(files.length > 0 ? UPLOAD_ROUTES.stream : '/api/http/stream'),
    {
      method: 'POST',
      headers: webHeaders({
        ...(files.length > 0 ? {} : { 'content-type': 'application/json' }),
        accept: 'text/event-stream',
      }),
      body: files.length > 0 ? uploadForm(args, files) : JSON.stringify(args),
      signal: controller.signal,
    },
    (raw) => dispatchSseEvent(raw, handlers),
    () => controller.signal.aborted,
    (event) => handlers.onError?.(event),
    isTerminalSseEvent,
    sseEndFallback(controller),
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
    isTerminalWsEvent,
    wsClosedFallback,
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
