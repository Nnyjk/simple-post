/**
 * `sendRequest` 的集成测试（响应与传输层）：状态 / JSON 解析、超时、调用方中止、
 * 网络错误、体积上限截断与重定向行为。
 *
 * 请求形状见 `http-request-shape.test.ts`，认证与模板见 `http-auth-vars.test.ts`。
 * 服务端脚手架在 `http-server-harness.ts`。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import type { IncomingMessage, ServerResponse } from 'node:http';

import { sendRequest } from '../http';
import {
  captured,
  getBaseUrl,
  getServer,
  reply,
  scriptNext,
  send,
  settings,
  startHarness,
  stopHarness,
} from './http-server-harness';

before(startHarness);
after(stopHarness);
// ============================================================
// 7. 响应处理
// ============================================================

describe('sendRequest: response handling', () => {
  it('200 with JSON body → status, bodyJson populated', async () => {
    scriptNext({
      status: 200,
      headers: { 'content-type': 'application/json' },
      body: '{"code":0,"data":{"id":"x"}}',
    });
    const resp = await send();
    assert.equal(resp.status, 200);
    assert.deepEqual(resp.bodyJson, { code: 0, data: { id: 'x' } });
    assert.equal(resp.contentType, 'application/json');
  });

  it('200 with non-JSON body → bodyJson undefined', async () => {
    scriptNext({
      status: 200,
      headers: { 'content-type': 'text/plain' },
      body: 'plain text',
    });
    const resp = await send();
    assert.equal(resp.status, 200);
    assert.equal(resp.body, 'plain text');
    assert.equal(resp.bodyJson, undefined);
  });

  it('404 response surfaces the status (no throw)', async () => {
    scriptNext({
      status: 404,
      headers: { 'content-type': 'application/json' },
      body: '{"code":40400,"message":"not found"}',
    });
    const resp = await send();
    assert.equal(resp.status, 404);
    assert.deepEqual(resp.bodyJson, { code: 40400, message: 'not found' });
  });

  it('500 response surfaces the status', async () => {
    scriptNext({
      status: 500,
      headers: { 'content-type': 'application/json' },
      body: '{"code":50000,"message":"server error"}',
    });
    const resp = await send();
    assert.equal(resp.status, 500);
  });

  it('captures response headers in the record', async () => {
    scriptNext({
      status: 200,
      headers: {
        'content-type': 'application/json',
        'x-request-id': 'req_abc',
      },
      body: '{}',
    });
    const resp = await send();
    assert.equal(resp.headers['x-request-id'], 'req_abc');
  });
});

// ============================================================
// 8. 超时（settings.timeoutMs）
// ============================================================

describe('sendRequest: timeout', () => {
  it('returns status 0 / Timeout when server delays past settings.timeoutMs', async () => {
    scriptNext({ status: 200, delayMs: 1000, body: '{"too":"late"}' });
    const resp = await send({ settings: settings({ timeoutMs: 200 }) });
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Timeout');
    assert.match(resp.error ?? '', /timeout after 200ms/);
  });

  it('completes when server responds within timeout', async () => {
    scriptNext({ status: 200, body: '{"ok":1}' });
    const resp = await send({ settings: settings({ timeoutMs: 1000 }) });
    assert.equal(resp.status, 200);
  });
});

// ============================================================
// 9. 调用方发起的中止
// ============================================================

describe('sendRequest: caller abort', () => {
  it('returns status 0 / Aborted when caller aborts before response', async () => {
    scriptNext({ status: 200, delayMs: 200, body: '{}' });
    const ctrl = new AbortController();
    const promise = send({ signal: ctrl.signal });
    setTimeout(() => ctrl.abort(), 50);
    const resp = await promise;
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Aborted');
    assert.equal(resp.error, 'aborted');
  });

  it('still completes when caller has not aborted by response time', async () => {
    scriptNext({ status: 200, body: '{"ok":1}' });
    const ctrl = new AbortController();
    const resp = await send({ signal: ctrl.signal });
    assert.equal(resp.status, 200);
    assert.ok(!ctrl.signal.aborted);
  });
});

// ============================================================
// 10. 网络错误
// ============================================================

describe('sendRequest: network error', () => {
  it('returns status 0 / Network Error when port is unreachable', async () => {
    // 端口 1 是保留端口且上面没有监听，连接应当很快被拒。这里用更紧的超时让测试保持快速。
    const resp = await sendRequest({
      method: 'GET',
      url: 'http://127.0.0.1:1/api/x',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      settings: { timeoutMs: 1500, followRedirects: true, maxResponseSizeKb: 5120 },
    });
    assert.equal(resp.status, 0);
    assert.equal(resp.statusText, 'Network Error');
    assert.ok(resp.error, 'expected an error message');
  });
});

// ============================================================
// 11. 体积上限 / 截断
// ============================================================

describe('sendRequest: size cap', () => {
  it('truncates body when response exceeds settings.maxResponseSizeKb', async () => {
    const big = 'x'.repeat(10 * 1024); // 10 KB
    scriptNext({
      status: 200,
      headers: { 'content-type': 'text/plain' },
      body: big,
    });
    const resp = await send({
      settings: settings({ maxResponseSizeKb: 1 }), // 1 KB cap
    });
    assert.ok(resp.body.length <= 1024 + 64, `body too large: ${resp.body.length}`);
    // 截断由服务端的 `http_executor::truncate_body` 执行（两种形态调的是同一个 Rust
    // 函数），所以标记文本是 Rust 那边那个，而不是客户端旧的 `[truncated at N KB]`。
    assert.match(resp.body, /\[truncated at 1024 bytes by Tauri HTTP layer\]$/);
  });

  it('passes through when response is within cap', async () => {
    scriptNext({
      status: 200,
      headers: { 'content-type': 'text/plain' },
      body: 'small body',
    });
    const resp = await send({
      settings: settings({ maxResponseSizeKb: 64 }),
    });
    assert.equal(resp.body, 'small body');
    assert.ok(!resp.body.includes('[truncated'));
  });
});

// ============================================================
// 12. 重定向行为
// ============================================================

describe('sendRequest: redirect', () => {
  type ReqListener = (req: IncomingMessage, res: ServerResponse) => void;
  it('follows 302 when settings.followRedirects is true', async () => {
    // 第一个服务端：302 → /redirected。第二个服务端：200 + body。
    // node:test 在一个文件内顺序执行，所以可以在另一个临时端口上再叠一个本地
    // 服务端。为了让本测试自包含，这里改用重定向到同一个服务端（第一次请求之后它
    // 默认返回 200）。
    captured.length = 0;
    let firstCall = true;
    const server = getServer();
    const originalListeners = server.listeners('request').slice() as ReqListener[];
    server.removeAllListeners('request');
    server.on('request', ((req: IncomingMessage, res: ServerResponse) => {
      if (firstCall) {
        firstCall = false;
        res.statusCode = 302;
        res.setHeader('Location', `${getBaseUrl()}/redirected`);
        res.end();
        return;
      }
      let body = '';
      req.on('data', (c: Buffer) => (body += c.toString('utf-8')));
      req.on('end', () => {
        captured.push({
          method: req.method ?? 'GET',
          url: req.url ?? '',
          headers: req.headers as Record<string, string>,
          body,
        });
        reply(res, {
          status: 200,
          body: '{"followed":true}',
          headers: { 'content-type': 'application/json' },
        });
      });
    }) as ReqListener);

    const resp = await send({ settings: settings({ followRedirects: true }) });
    assert.equal(resp.status, 200);
    assert.deepEqual(resp.bodyJson, { followed: true });
    assert.ok(captured.some((c) => c.url === '/redirected'));

    // 把默认 handler 恢复回去，供本文件后续测试使用。
    server.removeAllListeners('request');
    for (const l of originalListeners) server.on('request', l);
  });

  it('surfaces 3xx when settings.followRedirects is false', async () => {
    captured.length = 0;
    scriptNext({
      status: 302,
      headers: { Location: 'http://elsewhere.example/' },
      body: '',
    });
    const resp = await send({ settings: settings({ followRedirects: false }) });
    assert.equal(resp.status, 302);
    assert.equal(resp.headers['location'], 'http://elsewhere.example/');
  });
});
