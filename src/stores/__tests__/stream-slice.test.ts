/**
 * `sseEvents` 一次调用同时承担两件事：合并事件、累加字节。SSE 的原始文本是分块到的，很多
 * 块里只有半条事件（甚至一条都没有），而 `stream-sessions.ts` 对每一块都要报字节数 ——
 * 所以「这次没有事件」不能把字节一起扔掉，否则面板上的「已接收」永远是 0。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createStreamSlice } from '../slices/stream-slice';
import type { SseEventEntry, TabStreamState } from '../slices/stream-slice';

const TAB = 'tab-1';

/** 只装 `lastStreams` 的最小 store，`set` 支持 zustand 的函数式更新与对象合并。 */
function makeHarness() {
  const bag = { lastStreams: {} as Record<string, TabStreamState> };
  const set = (partial: unknown) => {
    const next =
      typeof partial === 'function' ? (partial as (s: typeof bag) => unknown)(bag) : partial;
    if (next && typeof next === 'object') Object.assign(bag, next);
  };
  const slice = createStreamSlice(set as never, (() => bag) as never, {} as never);
  return { bag, slice };
}

function event(seq: number, data: string): SseEventEntry {
  return { seq, event: 'message', id: '', data };
}

describe('stream-slice 的 sseEvents', () => {
  it('空 entries 只累加字节，事件列表与计数不动', () => {
    const { bag, slice } = makeHarness();
    slice.beginSse(TAB, 1_000);

    slice.sseEvents(TAB, [], 12);
    slice.sseEvents(TAB, [], 30);

    const run = bag.lastStreams[TAB]!.sse!;
    assert.equal(run.bytes, 42);
    assert.equal(run.eventCount, 0);
    assert.deepEqual(run.events, []);
    // 只有字节的一次调用不该把相位从「连接中」推走。
    assert.equal(run.phase, 'connecting');
  });

  it('非空 entries 合并事件的同时照常累加字节', () => {
    const { bag, slice } = makeHarness();
    slice.beginSse(TAB, 1_000);

    slice.sseEvents(TAB, [], 9);
    slice.sseEvents(TAB, [event(0, 'hello')], 7);

    const run = bag.lastStreams[TAB]!.sse!;
    assert.equal(run.bytes, 16);
    assert.equal(run.eventCount, 1);
    assert.deepEqual(
      run.events.map((e) => e.data),
      ['hello'],
    );
  });

  it('没有建连过（run 为 null）时不凭空造一条运行', () => {
    const { bag, slice } = makeHarness();
    slice.sseEvents(TAB, [], 5);
    assert.deepEqual(bag.lastStreams, {});
  });
});

describe('stream-slice 的 wsConnecting', () => {
  it('开新连接时清掉上一次的重连倒计时，帧与重连开关原样留着', () => {
    const { bag, slice } = makeHarness();
    slice.beginWs(TAB);
    slice.setWsReconnect(TAB, true);
    slice.wsFrame(TAB, {
      type: 'frame',
      direction: 'in',
      kind: 'text',
      data: 'hi',
      bytes: 2,
      at: 1,
    });
    slice.wsRetry(TAB, 3, 1_700_000_000_000);

    slice.wsConnecting(TAB);

    const ws = bag.lastStreams[TAB]!.ws!;
    assert.equal(ws.phase, 'connecting');
    assert.equal(ws.retryAttempt, 0, '倒计时没清，「第 3 次重试」会挂在一条正在连的会话上');
    assert.equal(ws.retryAt, null);
    assert.equal(ws.frameCount, 1, '重连不该把已收到的帧丢掉');
    assert.equal(ws.reconnectEnabled, true);
  });

  it('没有这条流的视图时不凭空造一条', () => {
    const { bag, slice } = makeHarness();
    slice.wsConnecting(TAB);
    assert.deepEqual(bag.lastStreams, {});
  });
});
