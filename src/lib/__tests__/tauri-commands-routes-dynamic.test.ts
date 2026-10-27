/**
 * web 形态下 `invokeCmd` 的路径与请求体映射 —— 动态变量。
 *
 * 与 `tauri-commands-routes-env.test.ts` 同构，断言 `(method, url, body)`，好让路由的
 * 静默漂移响亮地失败。
 *
 * 定义是全局的，故这组路由都不带项目参数：名字在整个工作区里唯一。
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

  it('dynamicVariablesCreate → POST /api/dynamic-variables with a flat body', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesCreate, {
      name: 'sns',
      sourceEndpointId: 'e1',
      extractPath: '$.list',
      ttlSeconds: 120,
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables');
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.name, 'sns');
    assert.equal(body.sourceEndpointId, 'e1');
    assert.equal(body.extractPath, '$.list');
    assert.equal(body.ttlSeconds, 120);
    // 缺席的字段也要显式发 null：服务端按「null = 清除」解释它。
    assert.equal(body.sourceInlineJson, null);
    assert.equal(body.valueMode, null);
    assert.equal(body.sourceEnvironmentId, null);
    assert.equal(body.sourceBaseUrlDefinitionId, null);
    // 定义是全局的，线上契约里没有项目这一维。
    assert.equal(body.projectId, undefined);
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
    // 名字是全局的：解析不带项目，也不带环境。
    assert.equal(body.projectId, undefined);
    assert.equal(body.environmentId, undefined);
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
  });

  it('dynamicVariablesSetOverride 传 null 表示清除覆盖值', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesSetOverride, { id: 'd1', value: null });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.value, null);
  });

  it('dynamicVariablesRefresh → POST /api/dynamic-variables/{id}/refresh with an empty body', async () => {
    await invokeCmd(COMMANDS.dynamicVariablesRefresh, { id: 'd1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/dynamic-variables/d1/refresh');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {});
  });
});
