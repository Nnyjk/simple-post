/**
 * 桌面形态的流式传输：事件经 `tauri::ipc::Channel` 逐条推回渲染层。
 *
 * `stream_request` / `ws_connect` 的连接、读流与超时都在 Rust 里发生（与 `http_request` 同一个
 * reqwest executor），这里只做三件事：接事件、把取消/发送/关闭转发成对应命令、把命令失败收成
 * `error` 事件交给调用方——UI 没有错误边界，故绝不让 Promise 的 rejection 冒出去。
 */
import { Channel, invoke } from '@tauri-apps/api/core';

import { COMMANDS } from '../tauri-commands';
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

/**
 * 事件通道的命令参数名。Rust 侧两条命令都以这个 camelCase 参数收 `Channel`，改名时与
 * `src-tauri/src/commands/` 同一提交。
 */
const EVENT_CHANNEL_ARG = 'onEvent';

function asStreamError(error: unknown): StreamErrorEvent {
  return {
    type: 'error',
    kind: 'network',
    message: error instanceof Error ? error.message : String(error),
  };
}

export function openHttpStream(args: HttpStreamArgs, handlers: SseHandlers): HttpStreamHandle {
  const channel = new Channel<unknown>();
  channel.onmessage = (event) => dispatchSseEvent(event, handlers);

  void invoke(COMMANDS.streamRequest, { ...args, [EVENT_CHANNEL_ARG]: channel }).catch((error) =>
    handlers.onError?.(asStreamError(error)),
  );

  return {
    cancel(): void {
      // 用户点的就是「停」：后端已经没有这次流（或正在收尾）时，取消失败不改变任何界面状态。
      void invoke(COMMANDS.streamCancel, { streamId: args.streamId }).catch(() => {});
    },
  };
}

export function openWsSession(args: WsConnectArgs, handlers: WsHandlers): WsSessionHandle {
  const channel = new Channel<unknown>();
  channel.onmessage = (event) => dispatchWsEvent(event, handlers);

  void invoke(COMMANDS.wsConnect, {
    ...args,
    [EVENT_CHANNEL_ARG]: channel,
  }).catch((error) => handlers.onError?.(asStreamError(error)));

  return {
    async send(kind, data, base64): Promise<void> {
      // 失败要冒给面板：发送区的错误行显示的是这次调用的原因，比一条泛化的断开事件有用。
      await invoke(COMMANDS.wsSend, {
        sessionId: args.sessionId,
        kind,
        data,
        base64: base64 ?? null,
      });
    },
    async close(code, reason): Promise<void> {
      await invoke(COMMANDS.wsClose, {
        sessionId: args.sessionId,
        code: code ?? null,
        reason: reason ?? '',
      });
    },
  };
}
