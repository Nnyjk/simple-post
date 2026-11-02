/**
 * web 形态下 `invokeCmd` 的路径与请求体映射 —— 代理权限、MCP 守护进程与自动启动偏好，
 * 外加「每次调用都带 content-type」这条横切断言。
 *
 * 与 `tauri-commands-routes-tree.test.ts` 同构，断言 `(method, url, body)`。
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
  it('permissionsGet → GET /api/permissions/{projectId}', async () => {
    await invokeCmd(COMMANDS.permissionsGet, { projectId: 'p-1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/permissions/p-1');
  });

  it('permissionsSet → PUT /api/permissions/{projectId} body { permission }', async () => {
    await invokeCmd(COMMANDS.permissionsSet, {
      projectId: 'p-1',
      permission: 'readonly',
    });
    const c = last();
    assert.equal(c.method, 'PUT');
    assert.equal(c.url, '/api/permissions/p-1');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { permission: 'readonly' });
  });

  it('mcpDaemonRestart → POST /api/mcp/restart', async () => {
    await invokeCmd(COMMANDS.mcpDaemonRestart);
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/mcp/restart');
  });

  it('mcpDaemonLogTail → GET /api/mcp/logs', async () => {
    await invokeCmd(COMMANDS.mcpDaemonLogTail);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/mcp/logs');
    assert.equal(c.body, undefined);
  });

  it('mcpDaemonProxyPath → GET /api/mcp/proxy-path', async () => {
    await invokeCmd(COMMANDS.mcpDaemonProxyPath);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/mcp/proxy-path');
    assert.equal(c.body, undefined);
  });

  it('mcpAutoStartSet rewrites the body to { key, value }', async () => {
    await invokeCmd(COMMANDS.mcpAutoStartSet, { value: true });
    const c = last();
    assert.equal(c.method, 'PUT');
    assert.equal(c.url, '/api/preferences');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      key: 'mcp.auto_start',
      value: true,
    });
  });

  it('mcpAutoStartGet performs GET and plucks mcp.auto_start from the bulk response', async () => {
    installFetchMock({
      json: { 'mcp.auto_start': true, request_settings: { timeoutMs: 1 } },
      status: 200,
    });
    const v = await invokeCmd<boolean>(COMMANDS.mcpAutoStartGet);
    assert.equal(v, true);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/preferences');
  });

  it('mcpAutoStartGet resolves to false when the key is missing', async () => {
    installFetchMock({ json: {}, status: 200 });
    const v = await invokeCmd<boolean>(COMMANDS.mcpAutoStartGet);
    assert.equal(v, false);
  });

  it('every invoke call sets content-type: application/json', async () => {
    await invokeCmd(COMMANDS.environmentsSetModuleBaseurl, {
      envId: 'env1',
      moduleId: 'mod1',
      url: 'https://api.example.com',
      proxy: null,
    });
    const c = last();
    assert.equal(c.contentType, 'application/json');
  });
});
