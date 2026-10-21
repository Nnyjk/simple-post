/**
 * 发送请求时带上脚本上下文，并拿回脚本的产出。
 *
 * 后端一次发送会跑整条脚本链（装配 → 前置脚本 → 发出 → 测试脚本 → 变量回写），
 * 所以渲染层要做的只有两件事：把**端点 id**与**环境**送过去（后端据此装配脚本链、
 * 决定变量往哪写），以及把回来的脚本产出交给界面。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, installIpcMock, clearIpcMock } from '@/stores/__tests__/_browser-shim';
import { sendPreparedRequest } from '../http';
import { prepareRequest } from '../http-build';
import type { RequestSettings } from '@/stores/app-store';
import type { Environment, ScriptOutcome } from '../../types/domain';

const SETTINGS: RequestSettings = {
  timeoutMs: 5000,
  followRedirects: true,
  maxResponseSizeKb: 1024,
};

const ENV: Environment = {
  id: 'env-1',
  projectId: 'p-1',
  name: 'dev',
  baseUrls: {},
  moduleBaseUrls: {},
  variables: { token: 'abc' },
  disabledVariables: [],
  isActive: true,
};

let shim: ReturnType<typeof installBrowserShim>;

beforeEach(() => {
  shim = installBrowserShim();
});

afterEach(() => {
  clearIpcMock();
  shim.teardown();
});

/** 后端一次发送的返回形状。 */
function serverResponse(script: ScriptOutcome | null) {
  return {
    http: {
      ok: true,
      response: {
        status: 200,
        statusText: 'OK',
        durationMs: 3,
        sizeBytes: 2,
        headers: [['content-type', 'application/json']],
        body: '{}',
        contentType: 'application/json',
        method: 'GET',
        url: 'http://x.test/y',
        truncated: false,
      },
    },
    script,
  };
}

describe('发送时带上脚本上下文', () => {
  it('把端点 id 与环境送过去，并拿回脚本的产出', async () => {
    const calls: { cmd: string; args: Record<string, unknown> }[] = [];
    const script: ScriptOutcome = {
      logs: [],
      assertions: [{ name: '状态码是 200', passed: true, error: null }],
      errors: [],
      envWrites: [{ key: 'next', value: 'v1' }],
    };
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return serverResponse(script);
    });

    const request = prepareRequest({
      method: 'GET',
      urlPath: '/y',
      baseUrl: 'http://x.test',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      env: ENV,
    });

    const result = await sendPreparedRequest({
      request,
      settings: SETTINGS,
      endpointId: 'ep-1',
      env: { id: ENV.id, variables: ENV.variables },
      preRequestScript: "pm.environment.set('a','1');",
      testScript: "pm.test('t', function () {});",
    });

    assert.equal(calls.length, 1);
    assert.equal(calls[0].cmd, 'http_request');
    assert.equal(calls[0].args.endpointId, 'ep-1');
    assert.deepEqual(calls[0].args.env, { id: 'env-1', variables: { token: 'abc' } });
    // 端点自己那两段脚本：送的是界面上此刻生效的那份，后端拿它覆盖存的那份。
    assert.equal(calls[0].args.preRequestScript, "pm.environment.set('a','1');");
    assert.equal(calls[0].args.testScript, "pm.test('t', function () {});");

    assert.equal(result.response.status, 200);
    assert.equal(result.script, script);
  });

  it('没有端点时送 null，没有脚本时拿回 null', async () => {
    const calls: { cmd: string; args: Record<string, unknown> }[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return serverResponse(null);
    });

    const request = prepareRequest({
      method: 'GET',
      urlPath: '/y',
      baseUrl: 'http://x.test',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
    });

    const result = await sendPreparedRequest({ request, settings: SETTINGS });

    assert.equal(calls[0].args.endpointId, null);
    assert.equal(calls[0].args.env, null);
    assert.equal(calls[0].args.preRequestScript, null);
    assert.equal(calls[0].args.testScript, null);
    assert.equal(result.script, null);
  });
});
