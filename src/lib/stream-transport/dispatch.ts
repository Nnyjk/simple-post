/**
 * 事件 DTO → 处理函数的唯一分派点。两种分发形态（Tauri Channel / Web 读取流）拿到的都是
 * 反序列化后的 JSON，故按 `type` 判别只写这一份。
 *
 * 每条事件先核对判别字段再交给处理函数：后端与前端是两个进程，坏消息不该在 UI 深处炸成
 * 「只能看到一条报错」的样子。未知 `type` 直接忽略——后端先加一种事件时，旧前端不该因此报错。
 */
import type {
  SseChunkEvent,
  SseEndEvent,
  SseHandlers,
  SseMetaEvent,
  StreamErrorEvent,
  WsClosedEvent,
  WsFrameEvent,
  WsHandlers,
  WsStateEvent,
} from './types';

function asRecord(raw: unknown): Record<string, unknown> | null {
  return typeof raw === 'object' && raw !== null ? (raw as Record<string, unknown>) : null;
}

function asErrorEvent(raw: Record<string, unknown>): StreamErrorEvent | null {
  if (typeof raw.message !== 'string') return null;
  const { kind } = raw;
  if (kind === 'timeout' || kind === 'network' || kind === 'protocol') {
    return { type: 'error', kind, message: raw.message };
  }
  // 后端给了没见过的 `kind` 时按网络错误收口，报文仍带后端原话。
  return { type: 'error', kind: 'network', message: raw.message };
}

export function dispatchSseEvent(raw: unknown, handlers: SseHandlers): void {
  const record = asRecord(raw);
  if (record === null) return;

  if (record.type === 'meta') {
    if (typeof record.status !== 'number') return;
    // 判别字段与它决定必有的那几个字段都核对过，其余字段由共享 DTO 契约保证。
    const event = record as unknown as SseMetaEvent;
    handlers.onMeta?.(event);
    return;
  }
  if (record.type === 'chunk') {
    if (typeof record.text !== 'string') return;
    // 判别字段与它决定必有的那几个字段都核对过，其余字段由共享 DTO 契约保证。
    const event = record as unknown as SseChunkEvent;
    handlers.onChunk?.(event);
    return;
  }
  if (record.type === 'end') {
    if (typeof record.durationMs !== 'number') return;
    // 判别字段与它决定必有的那几个字段都核对过，其余字段由共享 DTO 契约保证。
    const event = record as unknown as SseEndEvent;
    handlers.onEnd?.(event);
    return;
  }
  if (record.type === 'error') {
    const event = asErrorEvent(record);
    if (event !== null) handlers.onError?.(event);
  }
}

export function dispatchWsEvent(raw: unknown, handlers: WsHandlers): void {
  const record = asRecord(raw);
  if (record === null) return;

  if (record.type === 'state') {
    if (record.state !== 'connecting' && record.state !== 'open') return;
    // 判别字段与它决定必有的那几个字段都核对过，其余字段由共享 DTO 契约保证。
    const event = record as unknown as WsStateEvent;
    handlers.onState?.(event);
    return;
  }
  if (record.type === 'frame') {
    if (record.direction !== 'in' && record.direction !== 'out') return;
    if (record.kind !== 'text' && record.kind !== 'binary') return;
    // 判别字段与它决定必有的那几个字段都核对过，其余字段由共享 DTO 契约保证。
    const event = record as unknown as WsFrameEvent;
    handlers.onFrame?.(event);
    return;
  }
  if (record.type === 'closed') {
    if (typeof record.reason !== 'string') return;
    // 判别字段与它决定必有的那几个字段都核对过，其余字段由共享 DTO 契约保证。
    const event = record as unknown as WsClosedEvent;
    handlers.onClosed?.(event);
    return;
  }
  if (record.type === 'error') {
    const event = asErrorEvent(record);
    if (event !== null) handlers.onError?.(event);
  }
}
