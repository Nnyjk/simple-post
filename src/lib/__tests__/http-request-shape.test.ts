/**
 * `sendRequest` 的集成测试（请求形状）：对一个真实的本地 HTTP 服务跑 `fetch`，
 * 断言方法、查询参数、header 与 body 在线上那一侧的样子。
 *
 * 响应处理、超时 / 中止 / 网络错误、体积上限与重定向在同目录的其它分片里。
 * 服务端脚手架在 `http-server-harness.ts`。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import type { HttpMethod } from '../../types/domain';

import { captured, lastRequest, send, startHarness, stopHarness } from './http-server-harness';

before(startHarness);
after(stopHarness);
// ============================================================
// 1. 全部 7 个 HTTP 方法
// ============================================================

describe('sendRequest: HTTP methods', () => {
  const METHODS: HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

  for (const m of METHODS) {
    it(`${m} arrives at the server`, async () => {
      captured.length = 0;
      const resp = await send({ method: m });
      assert.equal(resp.status, 200);
      assert.equal(lastRequest().method, m);
    });
  }
});

// ============================================================
// 2. 查询参数
// ============================================================

describe('sendRequest: query params', () => {
  it('appends enabled params', async () => {
    captured.length = 0;
    await send({ params: [{ id: 'p1', key: 'page', value: '1', enabled: true }] });
    assert.equal(lastRequest().url, '/api/x?page=1');
  });

  it('joins multiple params with &', async () => {
    captured.length = 0;
    await send({
      params: [
        { id: 'p1', key: 'page', value: '1', enabled: true },
        { id: 'p2', key: 'size', value: '20', enabled: true },
      ],
    });
    assert.equal(lastRequest().url, '/api/x?page=1&size=20');
  });

  it('skips disabled params', async () => {
    captured.length = 0;
    await send({
      params: [
        { id: 'p1', key: 'a', value: '1', enabled: true },
        { id: 'p2', key: 'b', value: '2', enabled: false },
      ],
    });
    assert.equal(lastRequest().url, '/api/x?a=1');
  });
});

// ============================================================
// 3. Header + 自动 Content-Type + 自动 Accept
// ============================================================

describe('sendRequest: headers', () => {
  it('user header reaches the server', async () => {
    captured.length = 0;
    await send({ headers: [{ id: 'h1', key: 'X-Trace', value: 'abc', enabled: true }] });
    assert.equal(lastRequest().headers['x-trace'], 'abc');
  });

  it('auto-injects Content-Type for json body', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1}' },
    });
    assert.equal(lastRequest().headers['content-type'], 'application/json');
  });

  it('auto-injects Content-Type for form body', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'form', content: 'a=1&b=2' },
    });
    assert.equal(lastRequest().headers['content-type'], 'application/x-www-form-urlencoded');
  });

  it('auto-injects Content-Type for raw body', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'raw', content: 'hello world' },
    });
    assert.equal(lastRequest().headers['content-type'], 'text/plain');
  });

  it('does NOT inject Content-Type when body.mode is none', async () => {
    captured.length = 0;
    await send({ method: 'GET' });
    assert.equal(lastRequest().headers['content-type'], undefined);
  });

  it('auto-injects Accept: application/json', async () => {
    captured.length = 0;
    await send({ method: 'GET' });
    assert.equal(lastRequest().headers['accept'], 'application/json');
  });
});

// ============================================================
// 4. body 内容到达服务端
// ============================================================

describe('sendRequest: body', () => {
  it('JSON body arrives as the raw string', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'json', content: '{"a":1,"b":"x"}' },
    });
    assert.equal(lastRequest().body, '{"a":1,"b":"x"}');
  });

  it('form body arrives as the raw string', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'form', content: 'a=1&b=2' },
    });
    assert.equal(lastRequest().body, 'a=1&b=2');
  });

  it('raw body arrives as the raw string', async () => {
    captured.length = 0;
    await send({
      method: 'POST',
      body: { mode: 'raw', content: 'hello world' },
    });
    assert.equal(lastRequest().body, 'hello world');
  });

  it('mode=none sends no body even for POST', async () => {
    captured.length = 0;
    await send({ method: 'POST', body: { mode: 'none', content: 'should not be sent' } });
    assert.equal(lastRequest().body, '');
  });

  it('mode=json with empty content sends no body', async () => {
    captured.length = 0;
    await send({ method: 'POST', body: { mode: 'json', content: '' } });
    assert.equal(lastRequest().body, '');
  });
});
