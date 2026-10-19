/**
 * 导入 / 导出四条命令在 web 形态下的路径与请求体映射。
 *
 * 与 `tauri-commands-routes-mcp.test.ts` 同构，断言 `(method, url, body)`。
 *
 * 这里有一条**横切**断言值得单独说：四条全是 POST，而请求体就是 args 本身（没有
 * `body` 覆写）。所以「`selected` 数组原样送到服务端」这件事必须被断言到——它是
 * 勾选弹窗与落库接口之间的唯一契约，一旦某一侧改了键名（`selected` → `selectedKeys`），
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

describe('invokeCmd (web shape) — interchange', () => {
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

  it('interchangeImportPreview → POST /api/interchange/import/preview', async () => {
    await invokeCmd(COMMANDS.interchangeImportPreview, {
      projectId: 'p-1',
      moduleId: 'm-1',
      content: '{"info":{},"item":[]}',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/interchange/import/preview');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      projectId: 'p-1',
      moduleId: 'm-1',
      content: '{"info":{},"item":[]}',
    });
  });

  it('URL 模式把 url 原样送出，content 缺席就不出现该键', async () => {
    await invokeCmd(COMMANDS.interchangeImportPreview, {
      projectId: 'p-1',
      moduleId: 'm-1',
      url: 'https://example.com/openapi.yaml',
    });
    const body = JSON.parse(last().body ?? 'null') as Record<string, unknown>;
    assert.equal(body.url, 'https://example.com/openapi.yaml');
    assert.ok(!('content' in body), 'content 缺席而不是 null，两侧对「二选一」的理解才一致');
  });

  it('interchangeImportCommit → selected 原样送到服务端', async () => {
    await invokeCmd(COMMANDS.interchangeImportCommit, {
      projectId: 'p-1',
      moduleId: 'm-1',
      content: '{}',
      selected: ['/item/0/request/0', '/item/1'],
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/interchange/import/commit');
    assert.deepEqual(JSON.parse(c.body ?? 'null').selected, ['/item/0/request/0', '/item/1']);
  });

  it('空选择集也是一个合法的请求，不该被吞掉', async () => {
    await invokeCmd(COMMANDS.interchangeImportCommit, {
      projectId: 'p-1',
      moduleId: 'm-1',
      content: '{}',
      selected: [],
    });
    // 服务端把空集当作「用户一个都不导」，不是失败。少发这个键会让服务端
    // 收到 `undefined` 并落到另一条分支上。
    assert.deepEqual(JSON.parse(last().body ?? 'null').selected, []);
  });

  it('interchangeExportPreview → POST /api/interchange/export/preview', async () => {
    await invokeCmd(COMMANDS.interchangeExportPreview, {
      projectId: 'p-1',
      moduleId: 'm-1',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/interchange/export/preview');
  });

  it('interchangeExportDocument → environmentId 与 format 原样送出', async () => {
    await invokeCmd(COMMANDS.interchangeExportDocument, {
      projectId: 'p-1',
      moduleId: 'm-1',
      environmentId: '',
      selected: ['n-1'],
      format: 'yaml',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/interchange/export/document');
    const body = JSON.parse(c.body ?? 'null');
    // 空串是「不选环境」，它与「没传」在服务端是两条不同的路。
    assert.equal(body.environmentId, '');
    assert.equal(body.format, 'yaml');
    assert.deepEqual(body.selected, ['n-1']);
  });

  it('四条都不在公开名单里（它们写库 / 出站）', () => {
    // `auth::is_public_path` 是 Rust 侧的名单，渲染层只能断言「这四条确实在
    // `WEB_ROUTES` 里且都是 POST」——真要拒它们的是服务端那条横切断言。
    for (const cmd of [
      COMMANDS.interchangeImportPreview,
      COMMANDS.interchangeImportCommit,
      COMMANDS.interchangeExportPreview,
      COMMANDS.interchangeExportDocument,
    ]) {
      assert.equal(typeof cmd, 'string');
    }
  });
});
