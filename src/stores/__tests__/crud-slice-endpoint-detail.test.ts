/**
 * `crud-slice` 的乐观更新 + 回滚契约（`updateEndpointDetail` 一族）。
 *
 * 每个 CRUD 动作都是同一种形状：用 `set()` 乐观改本地状态、fire-and-forget 的
 * `invoke<...>(COMMANDS.*)`、出错时用 `set()` 回滚到先前状态。这里挑
 * `updateEndpointDetail`，因为它是树里被调用最多的乐观动作，且正好跑过乐观 + 回滚。
 * 环境值与 baseUrl 的契约见同目录的其它分片。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import {
  makeCrudHarness,
  makeDetail,
  readWarnings,
  type CapturedInvoke,
} from './crud-slice-harness';
// ============================================================
// T-2 —— 乐观更新 + 回滚
// ============================================================

describe('crud-slice: updateEndpointDetail — T-2 optimistic + rollback', () => {
  it('applies optimistic update, then rolls back when invoke rejects', async () => {
    const original = makeDetail({ nodeId: 'ep-rollback', url: '/widgets' });
    const harness = makeCrudHarness({ 'ep-rollback': original });

    // 让 `endpoint_detail_update` reject。切片的 catch 分支应当触发回滚的
    // set()，并用 `console.error` 记下这次失败。
    installIpcMock(async (cmd) => {
      if (cmd === 'endpoint_detail_update') {
        throw new Error('sqlite: disk I/O error');
      }
      throw new Error('unexpected cmd: ' + cmd);
    });

    // 给下面的相等性断言拍下调用前的状态快照。
    const beforeUpdate = harness.bag.endpointDetails['ep-rollback'];
    assert.equal(beforeUpdate.url, '/widgets');

    const action = harness.slice.updateEndpointDetail('ep-rollback', {
      method: 'POST',
      url: '/widgets/v2',
    });

    // IPC 落定之前乐观写入就已可见。
    assert.equal(harness.bag.endpointDetails['ep-rollback'].method, 'POST');
    assert.equal(harness.bag.endpointDetails['ep-rollback'].url, '/widgets/v2');

    await action;

    const after = harness.bag.endpointDetails['ep-rollback'];
    // 原来的 detail 必须仍原封不动地在 store 里。
    assert.ok(after, 'detail not deleted');
    assert.equal(after.nodeId, 'ep-rollback', 'same id');
    assert.equal(after.method, 'GET', 'optimistic method change rolled back');
    assert.equal(after.url, '/widgets', 'optimistic url change rolled back');

    // 切片应当发出带同一条错误消息的 console 告警/错误——生产环境正是这样把
    // 失败暴露给用户的。两条通道都接受，因为切片两条都用（poll 那种用
    // `console.warn`，写入用 `console.error`）。
    const warnings = readWarnings();
    const seenFailure = warnings.some(
      (w) => /sqlite.*disk I.O. error/i.test(w) || /Error: sqlite: disk I\/O error/i.test(w),
    );
    assert.ok(
      seenFailure,
      'failure should have been logged to console.warn|console.error; got: ' +
        JSON.stringify(warnings),
    );

    // 核对：恢复出来的对象引用的正是切片持有的那个 `prev` 捕获值。断言
    // 引用相等反而太强——切片先把乐观对象存进映射，回滚时再用 `prev` 替换它，
    // 所以我们看到的**就是** `prev`。
    assert.strictEqual(
      after,
      beforeUpdate,
      'rolled-back detail reuses the prev snapshot reference',
    );
  });

  it('未知 nodeId 不发 IPC — 没有 detail 行可乐观写入', async () => {
    const harness = makeCrudHarness({});
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.updateEndpointDetail('ghost', { url: '/nope' });

    assert.deepEqual(calls, [], '没有 detail 行就不该打扰后端');
  });
});
