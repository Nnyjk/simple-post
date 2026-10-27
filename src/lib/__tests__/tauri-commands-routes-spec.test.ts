/**
 * `specUpsert` 在 web 形态下的路径与请求体映射。
 *
 * 与 `tauri-commands-routes-interchange.test.ts` 同构，断言 `(method, url, body)`。
 *
 * 这里有一条**横切**断言值得单独说：请求体就是那份规格本身（没有外层包装），
 * 所以「规格里的字段原样送到服务端」这件事必须被断言到——它是 MCP 工具与这条
 * REST 路由之间唯一的契约，一旦某一侧改了键名（`projectId` → `project_id`），
 * 这里会红，而功能测试全都不会发现。
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

describe('invokeCmd (web shape) — interface specs', () => {
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

  it('specUpsert → POST /api/spec/upsert', async () => {
    await invokeCmd(COMMANDS.specUpsert, {
      spec: {
        projectId: 'p-1',
        module: '交易',
        collectionPath: '订单/明细',
        endpoint: { name: '查订单', method: 'GET', url: '/api/orders/{id}' },
      },
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/spec/upsert');
  });

  it('请求体就是那份规格本身，不带外层包装', async () => {
    const spec = {
      projectId: 'p-1',
      module: '交易',
      collectionPath: '订单/明细',
      environment: { name: 'dev', variables: { token: 'abc' } },
      service: { name: '网关' },
      endpoint: { name: '查订单', method: 'GET', url: '/api/orders/{id}' },
    };
    await invokeCmd(COMMANDS.specUpsert, { spec });
    const body = JSON.parse(last().body ?? 'null');
    assert.deepEqual(body, spec, '规格必须原样送出：包一层或多一层键，REST 侧就收不到它期望的形状');
    assert.ok(!('spec' in body), 'body 里不能还留着 `spec` 这一层包装');
  });
});
