/**
 * 流式传输的门面：桌面与 Web 两套实现在模块加载时选一次（与 `tauri-commands.ts` 同规矩），
 * 调用点只看到 `openHttpStream` / `openWsSession`。
 *
 * 两者都采用「先返回句柄、事件后到」的形状：id 由调用方铸造，故建连之前就能取消 / 关闭，
 * 面板不必等第一个事件才拿到控制手段。
 */
import { IS_TAURI_SHELL } from './tauri-commands';
import * as tauri from './stream-transport/tauri';
import * as web from './stream-transport/web';
import type {
  HttpStreamArgs,
  HttpStreamHandle,
  SseHandlers,
  WsConnectArgs,
  WsHandlers,
  WsSessionHandle,
} from './stream-transport/types';

const impl = IS_TAURI_SHELL ? tauri : web;

export function openHttpStream(args: HttpStreamArgs, handlers: SseHandlers): HttpStreamHandle {
  return impl.openHttpStream(args, handlers);
}

export function openWsSession(args: WsConnectArgs, handlers: WsHandlers): WsSessionHandle {
  return impl.openWsSession(args, handlers);
}

export type {
  HttpStreamArgs,
  HttpStreamHandle,
  SseChunkEvent,
  SseEndEvent,
  SseHandlers,
  SseMetaEvent,
  StreamErrorEvent,
  WsConnectArgs,
  WsFrameEvent,
  WsFrameKind,
  WsHandlers,
  WsSessionHandle,
  WsStateEvent,
  WsClosedEvent,
} from './stream-transport/types';
