import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { runBatch } from '../model/runner';
import type { BatchItem, BatchOutcome } from '../model/types';

function item(id: string): BatchItem {
  return { endpointId: id, name: id, method: 'GET', url: `/${id}` };
}

function outcome(id: string, ok: boolean, status = ok ? 200 : 500): BatchOutcome {
  return {
    endpointId: id,
    name: id,
    method: 'GET',
    success: ok,
    status,
    statusText: ok ? 'OK' : 'Error',
    durationMs: 1,
    failure: ok ? null : 'http',
  };
}

describe('runBatch', () => {
  it('按顺序发送，每条一次', async () => {
    const seen: string[] = [];
    const items = [item('a'), item('b'), item('c')];
    const result = await runBatch(
      items,
      async (it) => {
        seen.push(it.endpointId);
        return outcome(it.endpointId, true);
      },
      { signal: new AbortController().signal },
    );
    assert.deepEqual(seen, ['a', 'b', 'c'], '必须顺序发，不并发');
    assert.equal(result.outcomes.length, 3);
    assert.equal(result.report.success, 3);
    assert.equal(result.report.failed, 0);
    assert.equal(result.aborted, false);
  });

  it('单条失败不中断整批', async () => {
    const seen: string[] = [];
    const items = [item('a'), item('b'), item('c')];
    const result = await runBatch(
      items,
      async (it) => {
        seen.push(it.endpointId);
        return outcome(it.endpointId, it.endpointId !== 'b');
      },
      { signal: new AbortController().signal },
    );
    assert.deepEqual(seen, ['a', 'b', 'c'], 'b 失败之后 c 照发');
    assert.equal(result.report.success, 2);
    assert.equal(result.report.failed, 1);
  });

  it('send 抛异常时记成一条失败，整批照跑', async () => {
    const seen: string[] = [];
    const items = [item('a'), item('b')];
    const result = await runBatch(
      items,
      async (it) => {
        seen.push(it.endpointId);
        if (it.endpointId === 'a') throw new Error('组装炸了');
        return outcome(it.endpointId, true);
      },
      { signal: new AbortController().signal },
    );
    assert.deepEqual(seen, ['a', 'b']);
    assert.equal(result.outcomes[0].failure, 'transport');
    assert.match(result.outcomes[0].error ?? '', /组装炸了/);
    assert.equal(result.report.failed, 1);
    assert.equal(result.report.success, 1);
  });

  it('中止后不再开始新的请求，没轮到的那几条算未执行', async () => {
    const ctrl = new AbortController();
    const seen: string[] = [];
    const items = [item('a'), item('b'), item('c')];
    const result = await runBatch(
      items,
      async (it) => {
        seen.push(it.endpointId);
        // 第一条发完就中止，后面的不该再开始。
        if (it.endpointId === 'a') ctrl.abort();
        return outcome(it.endpointId, true);
      },
      { signal: ctrl.signal },
    );
    assert.deepEqual(seen, ['a'], 'b 与 c 不该开始');
    assert.equal(result.aborted, true);
    assert.equal(result.outcomes.length, 1);
    assert.equal(result.report.notRun, 2, 'b 与 c 是未执行，不是失败');
    assert.equal(result.report.failed, 0);
  });

  it('中止后仍开跑的那批不算失败', async () => {
    const ctrl = new AbortController();
    ctrl.abort();
    const result = await runBatch([item('a')], async () => outcome('a', true), {
      signal: ctrl.signal,
    });
    assert.equal(result.outcomes.length, 0);
    assert.equal(result.report.notRun, 1);
    assert.equal(result.report.failed, 0);
    assert.equal(result.aborted, true);
  });

  it('逐条回调：开始一条报一次，跑完一条报一次', async () => {
    const started: number[] = [];
    const done: string[] = [];
    await runBatch([item('a'), item('b')], async (it) => outcome(it.endpointId, true), {
      signal: new AbortController().signal,
      onStart: (i) => started.push(i),
      onOutcome: (o) => done.push(o.endpointId),
    });
    assert.deepEqual(started, [0, 1]);
    assert.deepEqual(done, ['a', 'b']);
  });
});
