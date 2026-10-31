import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { buildReport } from '../model/report';
import type { BatchOutcome } from '../model/types';

function outcome(partial: Partial<BatchOutcome> & { endpointId: string }): BatchOutcome {
  return {
    name: partial.endpointId,
    method: 'GET',
    success: true,
    status: 200,
    statusText: 'OK',
    durationMs: 10,
    failure: null,
    ...partial,
  };
}

describe('buildReport', () => {
  it('空批次：各项都是 0', () => {
    const report = buildReport(0, [], 0);
    assert.equal(report.total, 0);
    assert.equal(report.success, 0);
    assert.equal(report.failed, 0);
    assert.equal(report.notRun, 0);
    assert.deepEqual(report.statusCounts, []);
  });

  it('成功与失败分开数', () => {
    const report = buildReport(
      3,
      [
        outcome({ endpointId: 'a', success: true, status: 200 }),
        outcome({ endpointId: 'b', success: false, status: 500, failure: 'http' }),
        outcome({ endpointId: 'c', success: true, status: 204 }),
      ],
      100,
    );
    assert.equal(report.success, 2);
    assert.equal(report.failed, 1);
    assert.equal(report.notRun, 0);
  });

  it('状态码分布按状态码升序，只统计拿到状态码的那些', () => {
    const report = buildReport(
      4,
      [
        outcome({ endpointId: 'a', success: false, status: 500, failure: 'http' }),
        outcome({ endpointId: 'b', success: true, status: 200 }),
        outcome({ endpointId: 'c', success: false, status: 500, failure: 'http' }),
        outcome({ endpointId: 'd', success: true, status: 201 }),
      ],
      100,
    );
    assert.deepEqual(report.statusCounts, [
      { status: 200, count: 1 },
      { status: 201, count: 1 },
      { status: 500, count: 2 },
    ]);
  });

  it('压根没发出去的那几条不算「没拿到响应」', () => {
    const report = buildReport(
      2,
      [
        outcome({ endpointId: 'a', success: false, status: 0, failure: 'blocked' }),
        outcome({ endpointId: 'b', success: false, status: 0, failure: 'transport' }),
      ],
      100,
    );
    assert.equal(report.blocked, 1);
    assert.equal(report.noResponse, 1, '只有传输失败的那条算没拿到响应');
    assert.equal(report.failed, 2);
  });

  it('未执行的条数是总数减去跑完的条数', () => {
    const report = buildReport(5, [outcome({ endpointId: 'a' })], 100);
    assert.equal(report.notRun, 4);
    assert.equal(report.failed, 0, '未执行不是失败');
  });

  it('恒等式：总数 = 状态码分布 + 没拿到响应 + 没发出去 + 未执行', () => {
    const report = buildReport(
      6,
      [
        outcome({ endpointId: 'a', success: true, status: 200 }),
        outcome({ endpointId: 'b', success: false, status: 404, failure: 'http' }),
        outcome({ endpointId: 'c', success: false, status: 0, failure: 'transport' }),
        outcome({ endpointId: 'd', success: false, status: 0, failure: 'blocked' }),
      ],
      100,
    );
    const counted =
      report.statusCounts.reduce((sum, s) => sum + s.count, 0) +
      report.noResponse +
      report.blocked +
      report.notRun;
    assert.equal(counted, report.total, '四类必须互斥且穷尽，否则总数对不上');
  });

  it('总耗时是各条耗时之和', () => {
    const report = buildReport(
      2,
      [outcome({ endpointId: 'a', durationMs: 30 }), outcome({ endpointId: 'b', durationMs: 70 })],
      250,
    );
    assert.equal(report.totalDurationMs, 100);
    assert.equal(report.elapsedMs, 250);
  });

  it('结论条数超过总数时当场报错，不把差值压成 0 藏起来', () => {
    // 结论条数比总数多，说明调用方把两个批次的结果混在了一起。那种情况下报出来的
    // 总数必然对不上恒等式，故宁可报错，也不让它悄悄过去。
    assert.throws(
      () => buildReport(1, [outcome({ endpointId: 'a' }), outcome({ endpointId: 'b' })], 0),
      /超过了这一批的总数/,
    );
  });
});
