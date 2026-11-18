/**
 * WebSocket 掉线与自动重连的节奏。
 *
 * 面板只报「连上」与「掉了」，节奏全在这里：`1s → 2s → 4s → 8s → 16s`，连续失败 5 次后
 * 不再试。时钟与定时器由调用方注入，故这里用假时钟把整条序列走一遍，不睡真实时间。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  createReconnect,
  WS_BACKOFF_MS,
  WS_MAX_RETRIES,
  type ReconnectTimers,
} from '../stream/ws-reconnect';

/** 假时钟：`advance` 只跑触到点的定时器，跑完把时钟推到目标时刻。 */
function fakeClock(start = 1_000) {
  let now = start;
  let seq = 0;
  const tasks = new Map<number, { at: number; run: () => void }>();
  const timers: ReconnectTimers = {
    setTimeout(fn, ms) {
      seq += 1;
      tasks.set(seq, { at: now + ms, run: fn });
      return seq;
    },
    clearTimeout(handle) {
      tasks.delete(handle as number);
    },
  };
  return {
    timers,
    now: () => now,
    advance(ms: number) {
      const target = now + ms;
      for (;;) {
        const due = [...tasks.entries()]
          .filter(([, task]) => task.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        tasks.delete(due[0]);
        now = due[1].at;
        due[1].run();
      }
      now = target;
    },
    pending: () => tasks.size,
  };
}

function harness() {
  const clock = fakeClock();
  const connects: number[] = [];
  const schedules: Array<{ at: number | null; attempt: number }> = [];
  const reconnect = createReconnect({
    timers: clock.timers,
    now: clock.now,
    connect: () => connects.push(clock.now()),
    onSchedule: (at, attempt) => schedules.push({ at, attempt }),
  });
  return { clock, connects, schedules, reconnect };
}

describe('WebSocket 重连退避', () => {
  it('连续掉线按 1s → 2s → 4s → 8s → 16s 重试', () => {
    const { clock, connects, reconnect } = harness();
    const delays: number[] = [];
    for (let i = 0; i < WS_MAX_RETRIES; i += 1) {
      assert.equal(reconnect.onClosed(), true);
      const before = clock.now();
      clock.advance(WS_BACKOFF_MS[i]!);
      delays.push(connects[connects.length - 1]! - before);
    }
    assert.deepEqual(delays, [1_000, 2_000, 4_000, 8_000, 16_000]);
  });

  it('第 5 次之后不再安排，且退回 false', () => {
    const { clock, reconnect } = harness();
    for (let i = 0; i < WS_MAX_RETRIES; i += 1) {
      reconnect.onClosed();
      clock.advance(WS_BACKOFF_MS[i]!);
    }
    assert.equal(reconnect.isExhausted(), true);
    assert.equal(reconnect.onClosed(), false);
  });

  it('退避表封顶 30s', () => {
    assert.equal(WS_BACKOFF_MS[WS_BACKOFF_MS.length - 1], 30_000);
    assert.equal(WS_MAX_RETRIES, 5);
  });

  it('手动断开（dispose）后掉线不重试', () => {
    const { clock, connects, reconnect } = harness();
    reconnect.dispose();
    assert.equal(reconnect.onClosed(), false);
    clock.advance(60_000);
    assert.deepEqual(connects, []);
    assert.equal(clock.pending(), 0);
  });

  it('连上后失败计数清零，下次掉线又从 1s 起算', () => {
    const { clock, connects, reconnect } = harness();
    reconnect.onClosed();
    clock.advance(1_000);
    reconnect.onClosed();
    clock.advance(2_000);
    reconnect.onOpen();
    assert.equal(reconnect.isExhausted(), false);

    assert.equal(reconnect.onClosed(), true);
    const before = clock.now();
    clock.advance(1_000);
    assert.equal(connects[connects.length - 1]! - before, 1_000);
  });

  it('连上时取消待触发的重试，倒计时随之清空', () => {
    const { clock, connects, schedules, reconnect } = harness();
    reconnect.onClosed();
    reconnect.onOpen();
    assert.equal(clock.pending(), 0);
    assert.deepEqual(schedules[schedules.length - 1], { at: null, attempt: 0 });
    clock.advance(30_000);
    assert.deepEqual(connects, []);
  });

  it('关掉开关（cancel）后再打开可以从头重试', () => {
    const { clock, reconnect } = harness();
    for (let i = 0; i < WS_MAX_RETRIES; i += 1) {
      reconnect.onClosed();
      clock.advance(WS_BACKOFF_MS[i]!);
    }
    assert.equal(reconnect.isExhausted(), true);
    reconnect.cancel();
    assert.equal(reconnect.isExhausted(), false);
    assert.equal(reconnect.onClosed(), true);
  });

  it('掉线两次（旧连接的 closed 与 error 各来一条）只安排一次重试', () => {
    const { clock, connects, reconnect } = harness();
    assert.equal(reconnect.onClosed(), true);
    assert.equal(reconnect.onClosed(), false);
    clock.advance(1_000);
    assert.equal(connects.length, 1);
  });
});
