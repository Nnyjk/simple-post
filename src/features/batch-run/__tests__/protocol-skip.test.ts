/**
 * 批量运行按协议分流：SSE 与 WebSocket 不发。
 *
 * 这两类端点没有「一次请求换一次响应」的形状 —— SSE 会把连接挂到超时才收下一条空响应，
 * WebSocket 的 `ws://` 地址根本走不了 HTTP 发送。执行器主动跳过它们并计入报告的
 * 「未执行」那一档，报告的恒等式（总数 = 状态码分布之和 + 无响应 + 未发送 + 未执行）
 * 不能因此破掉。
 *
 * 走真实通路：`runBatch` → `sendBatchItem` → `/api/http/send` 桥（桩） → 本地 HTTP 服务。
 * 断言落在服务端收到几条请求，而不是构造器返回了什么。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

import { runBatch } from '../model/runner';
import { freezeEnvironments, sendBatchItem, type BatchSendContext } from '../model/send';
import type { BatchItem } from '../model/types';
import type { RequestSettings } from '@/stores/app-store';
import type {
  ApiResponse,
  EndpointDetail,
  Environment,
  ResourceNode,
  StoredRequestSnapshot,
} from '@/types/domain';

/** 服务端收到的请求方法，按到达顺序。 */
const received: string[] = [];

let server: Server;
let baseUrl: string;

before(async () => {
  server = createServer((req, res) => {
    received.push(`${req.method} ${req.url}`);
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end('{"ok":true}');
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  baseUrl = `http://127.0.0.1:${port}`;
});

after(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

interface SendBody {
  method: string;
  url: string;
}

const originalFetch: typeof fetch = globalThis.fetch.bind(globalThis);

/** `POST /api/http/send` 的桩：发真实上游请求，回 `http.ts` 期待的信封。 */
async function mockBridgeFetch(
  input: string | URL | Request,
  init?: RequestInit,
): Promise<Response> {
  const urlStr =
    typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;
  if (urlStr !== '/api/http/send') {
    return originalFetch(input as Request | string | URL, init);
  }

  const sendBody = JSON.parse(typeof init?.body === 'string' ? init.body : '{}') as SendBody;
  const upstream = await originalFetch(sendBody.url, { method: sendBody.method });
  const text = await upstream.text();

  return new Response(
    JSON.stringify({
      http: {
        ok: true,
        response: {
          status: upstream.status,
          statusText: upstream.statusText,
          durationMs: 1,
          sizeBytes: new Blob([text]).size,
          headers: [...upstream.headers.entries()],
          body: text,
          contentType: upstream.headers.get('content-type') ?? '',
          method: sendBody.method,
          url: upstream.url,
          truncated: false,
        },
      },
    }),
    { status: 200, headers: { 'content-type': 'application/json' } },
  );
}

globalThis.fetch = mockBridgeFetch as unknown as typeof fetch;

const SETTINGS: RequestSettings = {
  timeoutMs: 5000,
  followRedirects: true,
  maxResponseSizeKb: 0,
  tls: {
    skipVerify: false,
    caCertificatePath: null,
    clientCertificatePath: null,
    clientKeyPath: null,
  },
};

const PROJECT_ID = 'p1';
const MODULE_ID = 'm1';
const ENV_ID = 'env1';

function node(id: string, kind: ResourceNode['kind'], parentId: string | null): ResourceNode {
  return {
    id,
    rootId: PROJECT_ID,
    parentId,
    kind,
    name: id,
    sortOrder: 0,
    createdAt: 1,
    updatedAt: 1,
  };
}

const DETAILS: Record<string, EndpointDetail> = {
  http: detail('http', 'GET'),
  graphql: {
    ...detail('graphql', 'GET'),
    protocol: 'graphql',
    graphql: { query: '{ me }', variables: '{"id": 1}' },
  },
  sse: { ...detail('sse', 'GET'), protocol: 'sse' },
  websocket: { ...detail('websocket', 'GET'), protocol: 'websocket', url: '/ws' },
};

function detail(nodeId: string, method: EndpointDetail['method']): EndpointDetail {
  return {
    nodeId,
    method,
    url: '/orders',
    baseUrlOverride: null,
    params: [],
    pathParams: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    docs: '',
    notes: '',
    tags: [],
    responses: [],
    preRequestScript: '',
    testScript: '',
    tls: null,
  };
}

const ITEMS: BatchItem[] = Object.keys(DETAILS).map((id) => ({
  endpointId: id,
  name: id,
  method: 'GET',
  url: '/orders',
}));

function makeContext(): BatchSendContext {
  return {
    nodes: [
      node(MODULE_ID, 'module', null),
      ...Object.keys(DETAILS).map((id) => node(id, 'endpoint', MODULE_ID)),
    ],
    endpointDetails: DETAILS,
    environments: freezeEnvironments([env()]),
    activeEnvironmentId: ENV_ID,
    activeProjectId: PROJECT_ID,
    settings: SETTINGS,
    enabledDynamicNames: new Set<string>(),
    resolveDynamicVariables: async () => ({ values: {}, errors: [] }),
    addResponseHistory: (
      _endpointId: string,
      _response: ApiResponse,
      _snapshot?: StoredRequestSnapshot,
    ) => {},
  };
}

function env(): Environment {
  return {
    id: ENV_ID,
    projectId: PROJECT_ID,
    name: 'dev',
    moduleBaseUrls: { [MODULE_ID]: { url: baseUrl } },
    variables: {},
    disabledVariables: [],
    createdAt: 1,
  };
}

describe('批量运行：按协议分流', () => {
  it('SSE 与 WebSocket 跳过，报告计入「未执行」，恒等式仍然成立', async () => {
    received.length = 0;
    const ctx = makeContext();

    const { outcomes, report } = await runBatch(
      ITEMS,
      (item, signal) => sendBatchItem(item, ctx, signal),
      { signal: new AbortController().signal },
    );

    // 只有 HTTP 与 GraphQL 真的发出去了，且各自拿到了状态码。
    assert.equal(received.length, 2, `实际发出：${received.join(', ')}`);
    assert.deepEqual(received.sort(), ['GET /orders', 'POST /orders']);

    const byId = new Map(outcomes.map((o) => [o.endpointId, o]));
    assert.equal(byId.get('http')?.status, 200);
    assert.equal(byId.get('graphql')?.status, 200, 'GraphQL 照常按 HTTP 跑');

    for (const id of ['sse', 'websocket']) {
      const outcome = byId.get(id);
      assert.equal(outcome?.failure, 'skipped', `${id} 应当被跳过`);
      assert.equal(outcome?.status, 0);
      assert.ok((outcome?.error ?? '').length > 0, '跳过要带原因');
    }

    assert.equal(report.total, 4);
    assert.equal(report.success, 2);
    assert.equal(report.failed, 0, '主动跳过不是失败');
    assert.equal(report.notRun, 2);
    assert.equal(report.blocked, 0);
    assert.equal(report.noResponse, 0);

    const statusSum = report.statusCounts.reduce((sum, s) => sum + s.count, 0);
    assert.equal(
      statusSum + report.noResponse + report.blocked + report.notRun,
      report.total,
      '四类互斥且穷尽：状态码之和 + 无响应 + 未发送 + 未执行 = 总数',
    );
  });
});

describe('批量运行：GraphQL 也要过协议校验', () => {
  it('查询为空时按「未发送」拦下，一个请求都不出网', async () => {
    received.length = 0;
    const base = makeContext();
    const ctx: BatchSendContext = {
      ...base,
      nodes: [...base.nodes, node('graphql-empty', 'endpoint', MODULE_ID)],
      endpointDetails: {
        ...base.endpointDetails,
        'graphql-empty': {
          ...detail('graphql-empty', 'POST'),
          protocol: 'graphql',
          graphql: { query: '', variables: '' },
        },
      },
    };

    const { outcomes, report } = await runBatch(
      [{ endpointId: 'graphql-empty', name: 'graphql-empty', method: 'POST', url: '/orders' }],
      (item, signal) => sendBatchItem(item, ctx, signal),
      { signal: new AbortController().signal },
    );

    assert.equal(received.length, 0, '校验失败时不该有请求出网');
    assert.equal(outcomes[0]?.failure, 'blocked');
    assert.match(outcomes[0]?.error ?? '', /GraphQL 查询为空/);
    assert.equal(report.blocked, 1);
    assert.equal(report.notRun, 0);
  });
});
