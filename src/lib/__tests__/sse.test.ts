/**
 * 上游 SSE 文本的增量解析。
 *
 * 这一层只认文本，不碰网络：传输层交来的 chunk 可能切在任意位置，故半包、粘包、CRLF
 * 都是常态而非边界情况。事件列表要保留原始 `data` 文本，多行 `data:` 不能合成一行 ——
 * JSON 多行文本就是这样过线的。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createSseParser } from '../sse';

describe('SSE 解析', () => {
  it('多行 data 用换行连接成一条事件', () => {
    const parser = createSseParser();
    const events = parser.push('data: line1\ndata: line2\n\n');
    assert.deepEqual(events, [{ event: 'message', id: '', data: 'line1\nline2' }]);
  });

  it('event 与 id 进入事件，缺省事件名是 message', () => {
    const parser = createSseParser();
    const events = parser.push('event: update\nid: 42\ndata: {"a":1}\n\n');
    assert.deepEqual(events, [{ event: 'update', id: '42', data: '{"a":1}' }]);
  });

  it('注释行不产生事件，也不污染下一个事件的字段', () => {
    const parser = createSseParser();
    const events = parser.push(': keepalive\n\nevent: tick\ndata: 1\n\n');
    assert.deepEqual(events, [{ event: 'tick', id: '', data: '1' }]);
  });

  it('CRLF 分隔的行与空行同样成事件', () => {
    const parser = createSseParser();
    const events = parser.push('event: ping\r\ndata: pong\r\n\r\n');
    assert.deepEqual(events, [{ event: 'ping', id: '', data: 'pong' }]);
  });

  it('半包拼回来才成事件', () => {
    const parser = createSseParser();
    assert.deepEqual(parser.push('da'), []);
    assert.deepEqual(parser.push('ta: hi\n'), []);
    assert.deepEqual(parser.push('\n'), [{ event: 'message', id: '', data: 'hi' }]);
  });

  it('切在 CRLF 中间的半包不丢换行语义', () => {
    const parser = createSseParser();
    assert.deepEqual(parser.push('data: x\r'), []);
    assert.deepEqual(parser.push('\n\r\n'), [{ event: 'message', id: '', data: 'x' }]);
  });

  it('粘包时一段里凑齐多条事件按到达顺序交出', () => {
    const parser = createSseParser();
    const events = parser.push('data: a\n\ndata: b\n\n');
    assert.deepEqual(
      events.map((e) => e.data),
      ['a', 'b'],
    );
  });

  it('retry 不产生事件，被忽略', () => {
    const parser = createSseParser();
    const events = parser.push('retry: 5000\ndata: z\n\n');
    assert.deepEqual(events, [{ event: 'message', id: '', data: 'z' }]);
  });

  it('流结束时交出没有空行收尾的块', () => {
    const parser = createSseParser();
    assert.deepEqual(parser.push('data: tail'), []);
    assert.deepEqual(parser.flush(), [{ event: 'message', id: '', data: 'tail' }]);
  });

  it('流以孤立 CR 结尾：CR 是被截断的 CRLF 前半，不当数据', () => {
    const parser = createSseParser();
    // `push` 留着末尾的 CR 等下一段；流到这里就断了，下一半永远不会来。
    assert.deepEqual(parser.push('data: tail\r'), []);
    assert.deepEqual(parser.flush(), [{ event: 'message', id: '', data: 'tail' }]);
  });
});
