/**
 * 批量运行的环境快照隔离 —— 端到端测试。
 *
 * 盯住的不变式是：**这一批看到的是同一份变量**。脚本可以把变量写回环境，若第 1 条
 * 写回的变量出现在第 2 条的替换里，那份报告就不可复现了（同一个集合，两次跑出两个
 * 结果，且第二次的结果取决于第一次跑过没有）。
 *
 * 隔离靠两层保证，两层都要验：
 *   1. 批次开始时 `freezeEnvironments` 拍一份**深拷贝**，整批读的是那份拷贝；
 *   2. 脚本的写回由后端落库，界面在**整批跑完**之后才同步进本地副本。
 *
 * 本文件验第 1 层：第 1 条发完之后改动**活的环境对象**（这正是写回落库后 store 里那份
 * 会被替换掉的东西），第 2 条仍要用批次开始时的取值。
 *
 * 走的是真实通路：`sendBatchItem` → `prepareEndpointRequest` → `sendPreparedRequest`
 * → `/api/http/send` 桥 → 真实本地 HTTP 服务。断言落在**服务端收到什么**，而不是
 * 构造器返回什么 —— 只有后者挡不住「值组装对了却在去往线上的路上丢掉」。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

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

// ---------- 服务端脚手架 ----------

/** 服务端收到的请求路径，按到达顺序。 */
const receivedPaths: string[] = [];

let server: Server;
let baseUrl: string;

before(async () => {
  server = createServer((req, res) => {
    receivedPaths.push(req.url ?? '');
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ ok: true }));
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  baseUrl = `http://127.0.0.1:${port}`;
});

after(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

// ---------- 桥接桩 ----------

interface SendBody {
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: string | null;
}

const originalFetch: typeof fetch = globalThis.fetch.bind(globalThis);

/**
 * `POST /api/http/send` 的测试桩：剥掉信封、发真实上游请求、回 `http.ts` 期待的
 * `HttpResult` 信封。脚本不在桩里跑，故 `script` 缺席。
 */
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

// ---------- 夹具 ----------

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

const MODULE_ID = 'm1';
const PROJECT_ID = 'p1';
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

/** 模块 + 两个挂在它下面的接口。 */
const NODES: ResourceNode[] = [
  node(MODULE_ID, 'module', null),
  node('e1', 'endpoint', MODULE_ID),
  node('e2', 'endpoint', MODULE_ID),
];

function detail(id: string): EndpointDetail {
  return {
    nodeId: id,
    method: 'GET',
    // 变量在路径里：替换后的路径会出现在服务端收到的请求上。
    url: '/orders/{{token}}',
    baseUrlOverride: null,
    params: [],
    pathParams: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    docs: '',
    notes: '',
    tags: [],
    preRequestScript: '',
    testScript: '',
    tls: null,
  };
}

function item(id: string): BatchItem {
  return { endpointId: id, name: id, method: 'GET', url: '/orders/{{token}}' };
}

/** 活的环境对象：batch 开始时它会被 `freezeEnvironments` 拍一份副本。 */
function liveEnv(): Environment {
  return {
    id: ENV_ID,
    projectId: PROJECT_ID,
    name: 'dev',
    moduleBaseUrls: { [MODULE_ID]: { url: baseUrl } },
    variables: { token: 'v1' },
    disabledVariables: [],
    createdAt: 1,
  };
}

function makeContext(environments: readonly Environment[]): {
  ctx: BatchSendContext;
  history: { endpointId: string; snapshot?: StoredRequestSnapshot }[];
} {
  const history: { endpointId: string; snapshot?: StoredRequestSnapshot }[] = [];
  const ctx: BatchSendContext = {
    nodes: NODES,
    endpointDetails: { e1: detail('e1'), e2: detail('e2') },
    environments,
    activeEnvironmentId: ENV_ID,
    activeProjectId: PROJECT_ID,
    settings: SETTINGS,
    enabledDynamicNames: new Set<string>(),
    resolveDynamicVariables: async () => ({ values: {}, errors: [] }),
    addResponseHistory: (
      endpointId: string,
      _response: ApiResponse,
      snapshot?: StoredRequestSnapshot,
    ) => {
      history.push({ endpointId, snapshot });
    },
  };
  return { ctx, history };
}

// ---------- 用例 ----------

describe('批量运行：环境快照隔离', () => {
  it('第 1 条的写回不出现在第 2 条的替换里', async () => {
    receivedPaths.length = 0;
    const live = liveEnv();
    const { ctx } = makeContext(freezeEnvironments([live]));
    const signal = new AbortController().signal;

    const first = await sendBatchItem(item('e1'), ctx, signal);
    assert.equal(first.success, true, `第 1 条应当成功：${first.error ?? ''}`);

    // 脚本的写回落到库里之后，store 里那份活的环境对象会被换成新的取值。这一步就模拟
    // 它：批次读的那份拷贝不该跟着变。
    live.variables = { token: 'v2' };

    const second = await sendBatchItem(item('e2'), ctx, signal);
    assert.equal(second.success, true, `第 2 条应当成功：${second.error ?? ''}`);

    assert.deepEqual(
      receivedPaths,
      ['/orders/v1', '/orders/v1'],
      '第 2 条必须用批次开始时的取值，而不是写回之后的',
    );
  });

  it('快照是深拷贝：改动活对象不动副本', () => {
    const live = liveEnv();
    const [snapshot] = freezeEnvironments([live]);

    live.variables.token = 'v2';
    live.moduleBaseUrls[MODULE_ID] = { url: 'http://elsewhere.test' };
    live.disabledVariables.push('token');

    assert.equal(snapshot.variables.token, 'v1', '变量表是拷贝');
    assert.equal(snapshot.moduleBaseUrls[MODULE_ID].url, baseUrl, '「环境 × 模块」那一格是拷贝');
    assert.deepEqual(snapshot.disabledVariables, [], '停用集合是拷贝');
  });
});
