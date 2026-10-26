/**
 * 搜索控制器的行为 —— 防抖与作废标记。
 *
 * 这里钉的是「上一轮的结果不能盖掉这一轮」与「取消之后回调不再写状态」两条：它们都
 * 只在真界面上偶发（手快一点连敲几个字就能看到），故用假的定时器把时序摆出来。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createNodeSearch, type NodeSearchTimers } from '../node-search';

/** 一组可控的假定时器：定时器不会自己响，测试自己决定何时放它跑。 */
function fakeTimers() {
  const pending = new Map<number, () => void>();
  let nextId = 1;
  const timers: NodeSearchTimers = {
    set: (fn) => {
      const id = nextId++;
      pending.set(id, fn);
      return id;
    },
    clear: (handle) => {
      pending.delete(handle as number);
    },
  };
  return {
    timers,
    /** 还有几个定时器等着响。 */
    pendingCount: () => pending.size,
    /** 放跑当前那个定时器。 */
    flush: () => {
      const entries = [...pending.entries()];
      pending.clear();
      for (const [, fn] of entries) fn();
    },
  };
}

/** 一个由测试自己兑现的请求。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** 让挂起的 promise 回调跑完（微任务队列）。 */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe('搜索控制器', () => {
  it('连续输入只发一次请求', async () => {
    const clock = fakeTimers();
    const asked: string[] = [];
    const search = createNodeSearch<string>({
      run: async (text) => {
        asked.push(text);
        return [];
      },
      timers: clock.timers,
    });

    search.start('a');
    search.start('ab');
    search.start('abc');
    assert.equal(clock.pendingCount(), 1, '三次输入应当只留下一个待发的请求');

    clock.flush();
    await settle();
    assert.deepEqual(asked, ['abc']);
  });

  it('旧一轮的结果不覆盖新一轮的结果', async () => {
    const clock = fakeTimers();
    const first = deferred<string[]>();
    const second = deferred<string[]>();
    const asked: string[] = [];
    const results: string[][] = [];
    const search = createNodeSearch<string>({
      run: (text) => {
        asked.push(text);
        return text === 'a' ? first.promise : second.promise;
      },
      timers: clock.timers,
      onResults: (rows) => results.push(rows),
    });

    // 第一轮：请求已经在路上。
    search.start('a');
    clock.flush();
    assert.deepEqual(asked, ['a']);

    // 第二轮：请求也在路上。
    search.start('ab');
    clock.flush();
    assert.deepEqual(asked, ['a', 'ab']);

    // 第二轮先回，结果写进状态。
    second.resolve(['ab-result']);
    await settle();
    assert.deepEqual(results, [['ab-result']]);

    // 第一轮后回：它已经过期，不许再写。
    first.resolve(['a-result']);
    await settle();
    assert.deepEqual(results, [['ab-result']], '过期那一轮的结果不许覆盖新一轮');
  });

  it('取消之后回调不再写状态', async () => {
    const clock = fakeTimers();
    const pending = deferred<string[]>();
    const results: string[][] = [];
    const searching: boolean[] = [];
    const search = createNodeSearch<string>({
      run: () => pending.promise,
      timers: clock.timers,
      onResults: (rows) => results.push(rows),
      onSearching: (value) => searching.push(value),
    });

    search.start('a');
    clock.flush();
    assert.deepEqual(searching, [true]);

    search.cancel();
    pending.resolve(['late']);
    await settle();
    assert.deepEqual(results, [], '取消之后不许再写结果');
    assert.deepEqual(searching, [true], '取消之后不许再改搜索状态');
  });

  it('取消之后待发的请求不再发出', async () => {
    const clock = fakeTimers();
    const asked: string[] = [];
    const search = createNodeSearch<string>({
      run: async (text) => {
        asked.push(text);
        return [];
      },
      timers: clock.timers,
    });

    search.start('a');
    search.cancel();
    clock.flush();
    await settle();
    assert.deepEqual(asked, [], '取消之后那个待发的请求不必再发');
  });

  it('清空查询词会清掉结果并结束搜索状态', async () => {
    const clock = fakeTimers();
    const results: string[][] = [];
    const searching: boolean[] = [];
    const search = createNodeSearch<string>({
      run: async () => ['x'],
      timers: clock.timers,
      onResults: (rows) => results.push(rows),
      onSearching: (value) => searching.push(value),
    });

    search.start('a');
    clock.flush();
    await settle();
    assert.deepEqual(results, [['x']]);

    search.start('');
    assert.deepEqual(results, [['x'], []], '空查询词要把结果清空');
    assert.equal(searching.at(-1), false);
  });

  it('搜索失败时报告错误且不写结果', async () => {
    const clock = fakeTimers();
    const pending = deferred<string[]>();
    const results: string[][] = [];
    const errors: unknown[] = [];
    const search = createNodeSearch<string>({
      run: () => pending.promise,
      timers: clock.timers,
      onResults: (rows) => results.push(rows),
      onError: (error) => errors.push(error),
    });

    search.start('a');
    clock.flush();
    const failure = new Error('boom');
    pending.reject(failure);
    await settle();

    assert.deepEqual(results, [], '失败的一轮不写结果');
    assert.deepEqual(errors, [failure]);
  });

  it('过期的那一轮失败时也不报告错误', async () => {
    const clock = fakeTimers();
    const first = deferred<string[]>();
    const errors: unknown[] = [];
    const search = createNodeSearch<string>({
      run: (text) => (text === 'a' ? first.promise : Promise.resolve([])),
      timers: clock.timers,
      onError: (error) => errors.push(error),
    });

    search.start('a');
    clock.flush();
    search.start('ab');
    clock.flush();

    first.reject(new Error('boom'));
    await settle();
    assert.deepEqual(errors, [], '过期那一轮的失败不该报出来');
  });
});
