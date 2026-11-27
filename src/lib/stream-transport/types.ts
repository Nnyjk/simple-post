/**
 * 流式传输的公共形状：SSE 与 WebSocket 两种协议在桌面（Tauri Channel）与 Web（`text/event-stream`
 * 响应）上都用同一组 DTO。后端按 `type` 判别，前端只按 `type` 分派。
 *
 * 这些类型是前后端唯一的契约：字段名、枚举值一旦改动，Rust 侧的事件序列化与这里必须同一提交。
 */
import type { BodyMode, HttpMethod, TlsSettings } from '@/types/domain';

/** 一条 header 的线上形状：后端与 `http_request` 一样用元组列表（保留重名与顺序）。 */
export type HeaderTuples = Array<[string, string]>;

/**
 * 一次性 HTTP 流（SSE）的发起参数。
 *
 * 字段与 `POST /api/http/send` 同形，只多一个前端铸造的 `streamId`：建连之前就要能取消，
 * 故 id 不能等后端分配。
 */
export interface HttpStreamArgs {
  /** 前端用 `crypto.randomUUID()` 铸造；取消用它。 */
  streamId: string;
  method: HttpMethod;
  url: string;
  headers: HeaderTuples;
  /**
   * 请求体文本。语义随 `bodyMode` 变：`multipart` 是分片数组的 JSON 文本，`binary` 是文件
   * 描述的 JSON 文本（含 `path`），其余是原文。文件字节由执行层按路径现读。
   */
  body: string | null;
  bodyMode: BodyMode;
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
  /** HTTPS 出站的全局默认。 */
  tls: TlsSettings;
  /** 端点自己的 TLS 覆盖；没有就是 `null`。 */
  tlsOverride: TlsSettings | null;
}

/** 响应头到手时的一条事件：状态行、headers 与这次请求真正发出去的 header。 */
export interface SseMetaEvent {
  type: 'meta';
  status: number;
  statusText: string;
  headers: HeaderTuples;
  requestHeaders: HeaderTuples;
  contentType: string;
  url: string;
}

/** 一段原始流文本。`text` 是**上游字节解码后的原文**（可能是一段半包），由前端解析成事件。 */
export interface SseChunkEvent {
  type: 'chunk';
  seq: number;
  text: string;
  bytes: number;
}

/** 流的收尾。`stopped` = 用户按了停止，`timeout` = 触到 `timeoutMs`。 */
export interface SseEndEvent {
  type: 'end';
  durationMs: number;
  sizeBytes: number;
  truncated: boolean;
  reason: 'eof' | 'stopped' | 'timeout';
}

/** 建连或读取中途的失败。`kind` 决定用户看到的是超时、网络还是协议错误。 */
export interface StreamErrorEvent {
  type: 'error';
  kind: 'timeout' | 'network' | 'protocol';
  message: string;
}

export interface SseHandlers {
  onMeta?: (event: SseMetaEvent) => void;
  onChunk?: (event: SseChunkEvent) => void;
  onEnd?: (event: SseEndEvent) => void;
  onError?: (event: StreamErrorEvent) => void;
}

/** 取消一次 HTTP 流：关 socket，并把这次运行标记成 `stopped`。 */
export interface HttpStreamHandle {
  cancel(): void;
}

/** WS 握手参数。URL 与 headers 由调用方替换过变量后传入（传输层不再碰模板）。 */
export interface WsConnectArgs {
  /** 前端铸造；建连前就能关闭，故 id 不能等后端分配。 */
  sessionId: string;
  url: string;
  headers: HeaderTuples;
  /** 子协议；空数组 = 不协商。 */
  protocols: string[];
  /** 全局默认的 HTTPS 出站设置，与 HTTP 发送同一份。 */
  tls: TlsSettings;
  /** 端点自己那份，整体取代 `tls`；`null` = 继承全局。 */
  tlsOverride: TlsSettings | null;
}

export type WsStateName = 'connecting' | 'open';

/** 连接状态推进。`closed` 单独一条事件，故这里只有两个值。 */
export interface WsStateEvent {
  type: 'state';
  state: WsStateName;
}

export type WsFrameKind = 'text' | 'binary';
export type WsFrameDirection = 'in' | 'out';

/** 一帧。文本原样放在 `data`；二进制放 `base64`，`bytes` 是原始字节数。 */
export interface WsFrameEvent {
  type: 'frame';
  direction: WsFrameDirection;
  kind: WsFrameKind;
  data?: string;
  base64?: string;
  bytes: number;
  /** 毫秒时间戳。 */
  at: number;
}

/** 连接结束。`code` 由后端给出，异常断开时可能为 `null`。 */
export interface WsClosedEvent {
  type: 'closed';
  code: number | null;
  reason: string;
  at: number;
}

export interface WsHandlers {
  onState?: (event: WsStateEvent) => void;
  onFrame?: (event: WsFrameEvent) => void;
  onClosed?: (event: WsClosedEvent) => void;
  onError?: (event: StreamErrorEvent) => void;
}

/** 打开着的 WS 会话。`send` 只在 open 之后调用；两边都在服务端校验。 */
export interface WsSessionHandle {
  send(kind: WsFrameKind, data: string, base64?: string): Promise<void>;
  close(code?: number, reason?: string): Promise<void>;
}
