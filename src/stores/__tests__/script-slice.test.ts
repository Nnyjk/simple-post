/**
 * 请求脚本在前端这一侧的两条契约。
 *
 * ① 脚本写回的环境变量**只改本地副本**：落库由后端做（那是唯一那条持久化路径），
 *    前端把后端回传的「已应用」并进自己的副本，于是下一个请求立刻能解析到新变量。
 *    若前端也发一次写命令，就会出现两条写路径可能不一致——正是 #84 那类「副本陈旧」
 *    的坑。故这条断言是「**不发 IPC**」。
 *
 * ② 脚本的产出（断言 / 日志 / 错误 / 写回的变量）落进 store，与 `lastResponse` 同一刻
 *    写入，故两者永远描述同一次发送。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import { makeCrudHarness, makeEnvironment, type CapturedInvoke } from './crud-slice-harness';

import { createUiResponse } from '../slices/ui-response';
import type { ScriptOutcome } from '@/types/domain';

/** `ui-response` 切片要的最小 store。 */
function makeUiResponseHarness() {
  const bag = { lastResponse: null, lastScriptOutcome: null, requestPending: false };
  const set = (partial: unknown) => {
    const updater =
      typeof partial === 'function' ? (partial as (state: typeof bag) => unknown)(bag) : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = createUiResponse(set as never, get as never, {} as never);
  return { bag, slice };
}

describe('脚本写回的环境变量只并进本地副本', () => {
  it('不发 IPC，只改副本里的变量', () => {
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    const original = makeEnvironment({ variables: { token: 'abc' } });
    const harness = makeCrudHarness({}, [original]);

    harness.slice.applyEnvWrites('env-1', [
      { key: 'next', value: 'v1' },
      { key: 'token', value: null },
    ]);

    assert.deepEqual(
      harness.bag.environments[0].variables,
      { next: 'v1' },
      '写入与删除都应并进副本',
    );
    assert.deepEqual(calls, [], '落库由后端做，前端不再发一次写命令');
  });

  it('环境 id 对不上时什么都不做', () => {
    const original = makeEnvironment({ variables: { token: 'abc' } });
    const harness = makeCrudHarness({}, [original]);

    harness.slice.applyEnvWrites('env-other', [{ key: 'next', value: 'v1' }]);

    assert.deepEqual(harness.bag.environments[0].variables, { token: 'abc' });
  });
});

describe('脚本的产出落进 store', () => {
  it('与 lastResponse 同一刻写入', () => {
    const harness = makeUiResponseHarness();
    const outcome: ScriptOutcome = {
      logs: [{ level: 'log', message: 'hi' }],
      assertions: [{ name: '状态码是 200', passed: true, error: null }],
      errors: [],
      envWrites: [{ key: 'next', value: 'v1' }],
    };

    harness.slice.setLastScriptOutcome(outcome);

    assert.equal(harness.bag.lastScriptOutcome, outcome);
    assert.equal(harness.bag.lastResponse, null);
  });

  it('没有脚本时是 null', () => {
    const harness = makeUiResponseHarness();
    assert.equal(harness.bag.lastScriptOutcome, null);
    harness.slice.setLastScriptOutcome(null);
    assert.equal(harness.bag.lastScriptOutcome, null);
  });
});
