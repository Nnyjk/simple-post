/**
 * 请求协议决定「发送」按哪个模型走。
 *
 * HTTP 与 GraphQL 是一次请求/一次响应，走 `useRequestActions`；SSE 建流后由事件推着走；
 * WebSocket 是独立会话，连接按钮在面板里，Send 不参与。三者的界面各自绑各自的动作，
 * 这个映射是唯一的分叉点。
 */
import type { RequestProtocol } from '@/types/domain';

export type SendAction = 'http' | 'stream' | 'none';

export function sendActionFor(protocol: RequestProtocol): SendAction {
  switch (protocol) {
    case 'sse':
      return 'stream';
    case 'websocket':
      return 'none';
    default:
      // GraphQL 是 HTTP 语义层：同样是「发一次、拿一份响应」，只是请求体由组装器生成。
      return 'http';
  }
}
