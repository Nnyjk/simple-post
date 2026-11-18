/**
 * 协议决定「发送」走哪条路，以及 GraphQL 的线上形态。
 *
 * 界面上的一个 Send 按钮在三种协议下是三种动作：HTTP/GraphQL 发一次取一份响应，SSE 建一条
 * 流，WebSocket 根本不用它（连接按钮在面板里）。这里钉住分叉点，并钉住两条线上事实：
 * SSE 保留用户选的 GET，GraphQL 无论存的是什么方法都发 POST。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { prepareRequest } from '@/lib/http-build';
import { sendActionFor } from '../model/send-action';

const BASE_INPUT = {
  method: 'GET' as const,
  urlPath: '/events',
  baseUrl: '',
  params: [],
  headers: [],
  body: { mode: 'none' as const, content: '' },
  auth: { kind: 'none' as const },
  env: {
    id: 'env1',
    projectId: 'p1',
    name: 'dev',
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    createdAt: 1,
  },
};

describe('协议决定发送动作', () => {
  it('HTTP 与 GraphQL 走请求/响应，SSE 建流，WebSocket 不走 Send', () => {
    assert.equal(sendActionFor('http'), 'http');
    assert.equal(sendActionFor('graphql'), 'http');
    assert.equal(sendActionFor('sse'), 'stream');
    assert.equal(sendActionFor('websocket'), 'none');
  });

  it('SSE 不改用户选的方法', () => {
    const prepared = prepareRequest({ ...BASE_INPUT, protocol: 'sse' });
    assert.equal(prepared.method, 'GET');
    assert.equal(prepared.body, null);
  });

  it('GraphQL 存的是 GET 也发 POST', () => {
    const prepared = prepareRequest({
      ...BASE_INPUT,
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '' },
    });
    assert.equal(prepared.method, 'POST');
  });

  it('GraphQL 替换用户写过的 Content-Type 的值与位置', () => {
    const prepared = prepareRequest({
      ...BASE_INPUT,
      headers: [
        { id: 'h1', key: 'content-type', value: 'text/plain', enabled: true },
        { id: 'h2', key: 'X-Trace', value: 't1', enabled: true },
      ],
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '' },
    });
    assert.deepEqual(prepared.headers, [
      ['content-type', 'application/json'],
      ['X-Trace', 't1'],
      ['Accept', 'application/json'],
    ]);
  });

  it('GraphQL 没写过 Content-Type 时补在 Accept 之前', () => {
    const prepared = prepareRequest({
      ...BASE_INPUT,
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '' },
    });
    assert.deepEqual(prepared.headers, [
      ['Content-Type', 'application/json'],
      ['Accept', 'application/json'],
    ]);
  });
});
