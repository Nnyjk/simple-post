/**
 * 上游 SSE 文本的增量解析。
 *
 * 传输层交来的是**字节解码后的原文**（一段可能切在任意字符边界上的 chunk），这里把它拼成事件：
 * 多行 `data:` 用 `\n` 连接、`event`/`id` 各取最后一次、注释行与 `retry` 不产生事件。半包留在
 * 缓冲区等下一段，故调用方按到达顺序 `push` 即可，不必自己拼。
 *
 * `id` 只属于当前事件块，不做跨事件继承：事件列表是给人看的，把上一条的 id 挂在下一条上会
 * 读成「这条也带 id」。
 */

export interface SseEvent {
  /** 事件名；块里没写 `event:` 时按规范取 `message`。 */
  event: string;
  /** 块里写过的 `id:`；没写就是空串。 */
  id: string;
  /** 多行 `data:` 以 `\n` 连接后的原文（末尾那个换行符已去掉）。 */
  data: string;
}

export interface SseParser {
  /** 喂一段原文，返回这段里凑齐的事件。 */
  push(text: string): SseEvent[];
  /** 流结束时调用：处理没有终止换行的最后一行，并把尚未以空行收尾的块交出来。 */
  flush(): SseEvent[];
}

export function createSseParser(): SseParser {
  let buffer = '';
  let data = '';
  let event = '';
  let id = '';
  let pending: SseEvent[] = [];

  function dispatch(): void {
    if (data === '') {
      // 规范的「空块」不派发；字段也要丢掉，免得空块里的 event/id 粘到下一条。
      data = '';
      event = '';
      id = '';
      return;
    }
    pending.push({
      event: event || 'message',
      id,
      data: data.endsWith('\n') ? data.slice(0, -1) : data,
    });
    data = '';
    event = '';
    id = '';
  }

  function feedLine(raw: string): void {
    if (raw === '') {
      dispatch();
      return;
    }
    if (raw.startsWith(':')) return;
    const colon = raw.indexOf(':');
    const field = colon === -1 ? raw : raw.slice(0, colon);
    let value = colon === -1 ? '' : raw.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'data') data += `${value}\n`;
    else if (field === 'event') event = value;
    else if (field === 'id') id = value;
    // `retry` 与未知字段按规范忽略：重连节奏由本应用自己的退避决定。
  }

  function drain(): SseEvent[] {
    const out = pending;
    pending = [];
    return out;
  }

  return {
    push(text: string): SseEvent[] {
      buffer += text;
      let start = 0;
      for (let i = 0; i < buffer.length; i += 1) {
        const ch = buffer.charCodeAt(i);
        if (ch !== 10 && ch !== 13) continue;
        // 末尾的 CR 可能是被切开的 CRLF 的前半：留着，下一段到了再判。
        if (ch === 13 && i === buffer.length - 1) break;
        const raw = buffer.slice(start, i);
        if (ch === 13 && buffer.charCodeAt(i + 1) === 10) i += 1;
        start = i + 1;
        feedLine(raw);
      }
      buffer = buffer.slice(start);
      return drain();
    },

    flush(): SseEvent[] {
      if (buffer !== '') {
        // 末尾那个孤立的 CR 是被截断的 CRLF 的前半（`push` 特意留着它等下一段），不是
        // 数据：流到这里就结束，说明下一半永远不会来。丢掉它，其余照发。
        const trailing = buffer.endsWith('\r') ? buffer.slice(0, -1) : buffer;
        if (trailing !== '') feedLine(trailing);
        buffer = '';
      }
      dispatch();
      return drain();
    },
  };
}
