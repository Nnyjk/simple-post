/**
 * web 形态下 `invokeCmd` 的路径与请求体映射 —— 动态变量。
 *
 * 与 `tauri-commands-routes-env.test.ts` 同构，断言 `(method, url, body)`，好让路由的
 * 静默漂移响亮地失败。
 *
 * 定义挂在项目里，故这组路由带项目参数：列表与解析都能按项目过滤。
 */
import { describe, it, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';

import { COMMANDS, invokeCmd } from '../tauri-commands';
import {
  calls,
  installFetchMock,
  restoreFetchMock,
  type CapturedCall,
} from './tauri-commands-fetch-mock';

describe('invokeCmd (web shape) — 动态变量', () => {
  beforeEach(() => {
    installFetchMock({ json: { ok: true }, status: 200 });
  });

  after(() => {
    restoreFetchMock();
  });

  function last(): CapturedCall {
    assert.ok(calls.length > 0, 'fetch was not called');
    return calls[calls.length - 1] as CapturedCall;
  }

  it('dynamicVariablesList → GET /api/dynamic-variables with no query', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesList, {});
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/dynamic-variables');
  });

  it('dynamicVariablesList → GET /api/dynamic-variables?projectId=…', async () => {
    // 定义挂在项目里，故列表带一个可选的项目过滤。不带就是跨项目那一路。
    await invokeCmd(COMMANDS.dynamicVariablesList, { projectId: 'p1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/dynamic-variables?projectId=p1');
  });

  it('dynamicVariablesCreate → POST /api/dynamic-variables with a flat body', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesCreate, {
      projectId: 'p1',
      name: 'sns',
      sourceEndpointId: 'e1',
      extractPath: '$.list',
      transformScript: 'return pm.data.list;',
      ttlSeconds: 120,
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables');
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.name, 'sns');
    assert.equal(body.sourceEndpointId, 'e1');
    assert.equal(body.extractPath, '$.list');
    assert.equal(body.transformScript, 'return pm.data.list;');
    assert.equal(body.ttlSeconds, 120);
    // 缺席的字段也要显式发 null：服务端按「null = 清除」解释它。
    assert.equal(body.sourceInlineJson, null);
    assert.equal(body.valueMode, null);
    assert.equal(body.sourceEnvironmentId, null);
    // 归属项目是必填的那一维。
    assert.equal(body.projectId, 'p1');
  });

  it('dynamicVariablesUpdate → PATCH /api/dynamic-variables/{id} with the patch as body', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesUpdate, {
      id: 'd 1',
      patch: { name: 'factors', ttlSeconds: 30 },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/dynamic-variables/d%201');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'factors', ttlSeconds: 30 });
  });

  it('dynamicVariablesDelete → DELETE /api/dynamic-variables/{id}', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesDelete, { id: 'd1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/dynamic-variables/d1');
  });

  it('dynamicVariablesResolve → POST /api/dynamic-variables/resolve', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesResolve, {
      names: ['sns'],
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables/resolve');
    const body = JSON.parse(c.body ?? 'null');
    assert.deepEqual(body.names, ['sns']);
    assert.equal(body.force, false);
    // 不点名项目就是跨项目取值，故那一维发 null。
    assert.equal(body.projectId, null);
    // 调用方没有环境时这段也是 null：继承取数环境的定义回落到数据源项目的默认环境。
    assert.equal(body.environmentId, null);
  });

  it('dynamicVariablesResolve → 带上调用方选中的环境', async () => {
    // 继承取数环境的定义按「调用方此刻选中的环境」取数，故这段必须随命令一起送上去。
    await invokeCmd(COMMANDS.dynamicVariablesResolve, {
      names: ['sns'],
      projectId: 'p1',
      environmentId: 'e2',
    });
    const body = JSON.parse(last().body ?? 'null');
    assert.equal(body.environmentId, 'e2');
  });

  it('dynamicVariablesList → 取数环境进查询串', async () => {
    // 列表带回的当前值是某个环境下的那一格，故要说清问的是哪个环境。
    await invokeCmd(COMMANDS.dynamicVariablesList, { projectId: 'p1', environmentId: 'e2' });
    assert.equal(last().url, '/api/dynamic-variables?environmentId=e2&projectId=p1');
  });

  it('dynamicVariablesResolve → POST /api/dynamic-variables/resolve with a project', async () => {
    // 点名项目就是「只在这个项目里找」——请求发送走的就是这一路。
    await invokeCmd(COMMANDS.dynamicVariablesResolve, {
      names: ['sns'],
      projectId: 'p1',
    });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.projectId, 'p1');
  });

  it('dynamicVariablesSetOverride → POST /api/dynamic-variables/{id}/override', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesSetOverride, {
      id: 'd1',
      value: '[{"code":"A001"}]',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables/d1/override');
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.value, '[{"code":"A001"}]');
    assert.equal(body.forever, false);
    // 覆盖值落在 (定义 × 生效环境) 那一行，故这段也要说清是哪个环境。
    assert.equal(body.environmentId, null);
  });

  it('dynamicVariablesSetOverride 把调用方的环境一并送上去', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesSetOverride, {
      id: 'd1',
      value: 'x',
      environmentId: 'e2',
    });
    assert.equal(JSON.parse(last().body ?? 'null').environmentId, 'e2');
  });

  it('dynamicVariablesSetOverride 传 null 表示清除覆盖值', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesSetOverride, { id: 'd1', value: null });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.value, null);
  });

  it('dynamicVariablesRefresh → POST /api/dynamic-variables/{id}/refresh with the caller environment', async () => {
    // 「刷新」重取的正是当前环境下那一格，故调用方没有环境时这段发 null。
    await invokeCmd(COMMANDS.dynamicVariablesRefresh, { id: 'd1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables/d1/refresh');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { environmentId: null });
  });

  it('dynamicVariablesRefresh 带上调用方的环境', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesRefresh, { id: 'd1', environmentId: 'e2' });
    assert.equal(JSON.parse(last().body ?? 'null').environmentId, 'e2');
  });

  it('dynamicVariablesPreview → POST /api/dynamic-variables/preview with exactly the draft fields', async () => {
    // 预览发的是「表单里这一刻这份草稿」，不是一条已存的定义：故路径是字面量段 `/preview`，
    // body 里带的是草稿字段 + 调用方此刻选中的环境。
    await invokeCmd(COMMANDS.dynamicVariablesPreview, {
      name: 'sns',
      sourceEndpointId: 'e1',
      extractPath: '$.points',
      transformScript: 'return pm.data.length;',
      environmentId: 'e2',
      // 调用方多带的键不能跟着上车：线上契约就是下面那几个。
      somethingElse: 'nope',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables/preview');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      name: 'sns',
      sourceEndpointId: 'e1',
      sourceInlineJson: null,
      sourceEnvironmentId: null,
      extractPath: '$.points',
      transformScript: 'return pm.data.length;',
      environmentId: 'e2',
    });
  });

  it('dynamicVariablesPreview → 继承环境时 environmentId 与 sourceEnvironmentId 都是 null', async () => {
    // 两格是两件事：草稿没点名取数环境（继承）、调用方此刻也没有环境。
    await invokeCmd(COMMANDS.dynamicVariablesPreview, {
      sourceInlineJson: '{"method":"GET","url":"https://example.test/x"}',
      environmentId: '',
    });
    const body = JSON.parse(last().body ?? 'null');
    assert.equal(body.environmentId, null);
    assert.equal(body.sourceEnvironmentId, null);
    assert.equal(body.name, null);
    assert.equal(body.sourceEndpointId, null);
  });
});
