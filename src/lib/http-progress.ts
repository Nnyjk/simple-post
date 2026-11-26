/**
 * 带进度的发送：`http_request_progress` / `POST /api/http/send-progress` 的两套分发。
 *
 * 两种形态都先流式推若干 `progress` 帧，最后是 `done`（信封与 `/api/http/send` 相同）或
 * `error`。桌面走 Tauri `Channel`，Web 走 `fetch` + `ReadableStream`——`webInvokeCmd` 只会
 * 解析整段 JSON，读不了流。
 *
 * 这里**不**重试、不吞错：失败一律 reject，由 `http.ts::invokeAndMap` 折成
 * `status: 0` 的响应，与普通发送的错误路径一致。
 */
import { Channel, invoke } from '@tauri-apps/api/core';

import { COMMANDS, IS_TAURI_SHELL, readApiBase } from './tauri-commands';
import { readBearerFromStorage } from './tauri-commands/bearer';
import { WEB_ROUTES } from './tauri-commands/web-routes';

/** 传输进度快照。与 Rust 的 `HttpTransferProgress` 逐字段对应。 */
export interface HttpTransferProgress {
  sentBytes: number;
  totalBytes: number;
  serverReceivedBytes?: number;
  serverTotalBytes?: number;
}

/** 后端推回的一帧。`done.outcome` 与普通发送的返回同形。 */
interface HttpSendProgressEvent {
  type: 'progress' | 'done' | 'error';
  progress?: HttpTransferProgress;
  outcome?: unknown;
  message?: string;
}

/**
 * 发一次请求并沿途报告进度，返回终局信封（与 `invokeCmd(COMMANDS.httpRequest, …)` 同一个
 * 泛型形状）。
 */
export async function sendWithProgress<T>(
  args: Record<string, unknown>,
  signal: AbortSignal | undefined,
  onProgress: (progress: HttpTransferProgress) => void,
): Promise<T> {
  if (IS_TAURI_SHELL) {
    const channel = new Channel<HttpSendProgressEvent>();
    channel.onmessage = (event) => {
      if (event.type === 'progress' && event.progress) onProgress(event.progress);
    };
    // 终局走返回值而不是 `done` 帧：Tauri 命令本身有返回值，再推一帧会把同一份信封
    // 序列化两次。失败走 reject，与 `http_request` 一致。
    return await invoke<T>(COMMANDS.httpRequestProgress, { ...args, onEvent: channel });
  }
  return sendViaSse<T>(args, signal, onProgress);
}

/** 非 2xx 的响应体形如 `{"code","message"}`：把那句话拼进错误里。 */
async function httpFailure(response: Response, url: string): Promise<Error> {
  const base = `HTTP ${response.status} ${url}`;
  try {
    const parsed: unknown = JSON.parse(await response.text());
    if (
      typeof parsed === 'object' &&
      parsed !== null &&
      'message' in parsed &&
      typeof parsed.message === 'string' &&
      parsed.message.trim() !== ''
    ) {
      return new Error(`${base}：${parsed.message.trim()}`);
    }
  } catch {
    // 不是 JSON（代理页、纯文本）——保持原文。
  }
  return new Error(base);
}

/** 进度帧里的那个对象。字段是后端序列化出来的数字，非数字按缺席处理。 */
function asProgress(raw: unknown): HttpTransferProgress | undefined {
  if (typeof raw !== 'object' || raw === null) return undefined;
  if (!('sentBytes' in raw) || typeof raw.sentBytes !== 'number') return undefined;
  if (!('totalBytes' in raw) || typeof raw.totalBytes !== 'number') return undefined;
  return {
    sentBytes: raw.sentBytes,
    totalBytes: raw.totalBytes,
    serverReceivedBytes:
      'serverReceivedBytes' in raw && typeof raw.serverReceivedBytes === 'number'
        ? raw.serverReceivedBytes
        : undefined,
    serverTotalBytes:
      'serverTotalBytes' in raw && typeof raw.serverTotalBytes === 'number'
        ? raw.serverTotalBytes
        : undefined,
  };
}

/** 交付一整行 SSE 帧；不是 JSON 的行（心跳、代理插入的文本）丢掉。 */
function parseLine(line: string): HttpSendProgressEvent | null {
  const trimmed = line.endsWith('\r') ? line.slice(0, -1) : line;
  if (trimmed === '') return null;
  const text = trimmed.startsWith('data:') ? trimmed.slice(5).trimStart() : trimmed;
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof value !== 'object' || value === null || !('type' in value)) return null;
  const { type } = value;
  if (type !== 'progress' && type !== 'done' && type !== 'error') return null;
  return {
    type,
    progress: 'progress' in value ? asProgress(value.progress) : undefined,
    outcome: 'outcome' in value ? value.outcome : undefined,
    message: 'message' in value && typeof value.message === 'string' ? value.message : undefined,
  };
}

async function sendViaSse<T>(
  args: Record<string, unknown>,
  signal: AbortSignal | undefined,
  onProgress: (progress: HttpTransferProgress) => void,
): Promise<T> {
  const spec = WEB_ROUTES[COMMANDS.httpRequestProgress];
  if (!spec) {
    throw new Error(
      'Command "http_request_progress" 没有 REST 路由：补 src/lib/tauri-commands/web-routes.ts。',
    );
  }
  const url = readApiBase() + spec.path(args);
  const bearer = readBearerFromStorage();
  const response = await fetch(url, {
    method: spec.method,
    headers: {
      'content-type': 'application/json',
      ...(bearer ? { authorization: `Bearer ${bearer}` } : {}),
    },
    body: JSON.stringify(args),
    signal,
  });
  if (!response.ok) throw await httpFailure(response, url);
  if (response.body === null) throw new Error('发送进度流没有响应体');

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let outcome: T | null = null;

  /** 交付一整行，返回是否收到了终局。 */
  const emit = (line: string): boolean => {
    const event = parseLine(line);
    if (event === null) return false;
    if (event.type === 'progress') {
      if (event.progress) onProgress(event.progress);
      return false;
    }
    if (event.type === 'done') {
      // 终局信封由后端保证与 `/api/http/send` 同形，泛型 T 就是那个形状；运行期没有可校验的
      // 额外结构，故这一处用断言。
      outcome = event.outcome as T;
      return true;
    }
    throw new Error(event.message ?? '发送失败');
  };

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let newline = buffer.indexOf('\n');
    while (newline !== -1) {
      const terminal = emit(buffer.slice(0, newline));
      buffer = buffer.slice(newline + 1);
      if (terminal) return outcome as T;
      newline = buffer.indexOf('\n');
    }
  }
  // 流尾没有换行的最后一帧也要看：后端可能不补换行。
  buffer += decoder.decode();
  if (emit(buffer)) return outcome as T;
  throw new Error('发送进度流在终局事件之前结束');
}
