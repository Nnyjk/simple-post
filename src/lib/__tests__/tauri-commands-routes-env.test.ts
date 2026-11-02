/**
 * web 形态下 `invokeCmd` 的路径与请求体映射 —— 环境、模块地址、请求设置。
 *
 * 与 `tauri-commands-routes-tree.test.ts` 同构，断言 `(method, url, body)`，好让路由
 * 的静默漂移响亮地失败。
 */ import { describe, it, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';

import { COMMANDS, invokeCmd } from '../tauri-commands';
import {
  calls,
  installFetchMock,
  restoreFetchMock,
  type CapturedCall,
} from './tauri-commands-fetch-mock';
describe('invokeCmd (web shape) — path & body mapping', () => {
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
  it('environmentsList → GET /api/environments with projectId query', async () => {
    await invokeCmd(COMMANDS.environmentsList, { projectId: 'p1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/environments?projectId=p1');
  });

  it('environmentsSetVar → POST /api/environments/{envId}/vars with flat body', async () => {
    await invokeCmd(COMMANDS.environmentsSetVar, {
      envId: 'env 1',
      key: 'token',
      value: 'abc',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env%201/vars');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { key: 'token', value: 'abc' });
  });

  it('environmentsSetModuleBaseurl → POST /api/environments/{envId}/module-baseurls', async () => {
    await invokeCmd(COMMANDS.environmentsSetModuleBaseurl, {
      envId: 'env1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
      proxy: 'https://gw.example.com',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env1/module-baseurls');
    // 格子里有两个字段：模块地址与代理地址。它们各自独立，缺席即保持原值。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      moduleId: 'mod1',
      url: 'https://api.example.com',
      proxy: 'https://gw.example.com',
    });
  });

  it('environmentsUpdate → PATCH /api/environments/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.environmentsUpdate, {
      id: 'env-1',
      patch: { name: 'Renamed' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/environments/env-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('environmentsSetVarDisabled → POST /api/environments/{envId}/vars/disabled', async () => {
    await invokeCmd(COMMANDS.environmentsSetVarDisabled, {
      envId: 'env 1',
      key: 'token',
      disabled: true,
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env%201/vars/disabled');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { key: 'token', disabled: true });
  });

  it('environmentsRenameVar → POST /api/environments/{envId}/vars/rename with flat body', async () => {
    await invokeCmd(COMMANDS.environmentsRenameVar, {
      envId: 'env 1',
      fromKey: 'old',
      toKey: 'new',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    // 环境 id 走路径编码；重命名只需一次往返，不必先删后设。
    assert.equal(c.url, '/api/environments/env%201/vars/rename');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { fromKey: 'old', toKey: 'new' });
  });

  it('requestSettingsGet → GET /api/request-settings', async () => {
    await invokeCmd(COMMANDS.requestSettingsGet);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/request-settings');
  });

  it('requestSettingsUpdate → PUT /api/request-settings with full settings body', async () => {
    await invokeCmd(COMMANDS.requestSettingsUpdate, {
      patch: { timeoutMs: 5000, followRedirects: true, maxResponseSizeKb: 1024 },
    });
    const c = last();
    assert.equal(c.method, 'PUT');
    assert.equal(c.url, '/api/request-settings');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      timeoutMs: 5000,
      followRedirects: true,
      maxResponseSizeKb: 1024,
    });
  });
});
