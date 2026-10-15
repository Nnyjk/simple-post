/**
 * web 形态下 `invokeCmd` 的路径与请求体映射 —— 环境、baseUrl 服务定义、请求设置。
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
      defId: 'def1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/env1/module-baseurls');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      defId: 'def1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
    });
  });

  it('environmentsSetActive → POST /api/environments/active (literal route)', async () => {
    await invokeCmd(COMMANDS.environmentsSetActive, { envId: 'env-1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/environments/active');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { envId: 'env-1' });
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

  it('baseUrlDefsCreate → POST /api/projects/{projectId}/baseurls with { name }', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      // 服务挂在项目上，所以作用域是 projectId；顶层 `id` 不能随默认 args
      // 泄漏进 body（调用方把要用的 id 放在 opts 里）。
      id: 'def-1',
      projectId: 'p/1',
      name: 'Local',
      opts: { description: '本地服务' },
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/projects/p%2F1/baseurls');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      name: 'Local',
      description: '本地服务',
    });
  });

  it('baseUrlDefsCreate → opts.id 下发，服务行不会与 ★ / 单元格脱钩', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      projectId: 'p1',
      name: 'auth',
      opts: { id: 'def_client_1' },
    });
    const c = last();
    // 前端铸 id、后端采纳：丢掉它就等于让 ★ 和 env 单元格指向一条从未
    // 落库的行，解析必然为空。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'auth', id: 'def_client_1' });
  });

  it('baseUrlDefsCreate → opts.id 为空串时不发 id，由服务端铸', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsCreate, {
      projectId: 'p1',
      name: 'auth',
      opts: { id: '' },
    });
    const c = last();
    // 空串不能变成一条 id 为 "" 的行。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'auth' });
  });

  it('baseUrlDefsUpdate → PATCH /api/baseurl-defs/{id}', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsUpdate, {
      id: 'def-1',
      patch: { name: 'Renamed' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/baseurl-defs/def-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('baseUrlDefsDelete → DELETE /api/baseurl-defs/{id}', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsDelete, { id: 'def-1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/baseurl-defs/def-1');
    assert.equal(c.body, undefined);
  });

  it('baseUrlDefsSetDefault → POST /api/projects/{projectId}/baseurls/default with { defId } body', async () => {
    await invokeCmd(COMMANDS.baseUrlDefsSetDefault, {
      projectId: 'p-1',
      defId: 'd-2',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/projects/p-1/baseurls/default');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { defId: 'd-2' });
  });

  it('baseUrlDefsSetDefault → defId: null 表示该项目没有默认服务', async () => {
    // `null` 是清空 ★ 的显式动作，不是"恢复某个模块的默认值"——
    // ★ 是项目级的，根本不存在模块默认值。
    await invokeCmd(COMMANDS.baseUrlDefsSetDefault, { projectId: 'p-1', defId: null });
    const c = last();
    assert.equal(c.url, '/api/projects/p-1/baseurls/default');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { defId: null });
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
