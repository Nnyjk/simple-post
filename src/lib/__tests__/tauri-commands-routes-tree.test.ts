/**
 * web 形态下 `invokeCmd` 的路径与请求体映射 —— 项目与资源树（节点 + 接口详情）。
 *
 * 断言 `(method, url, 解析后的 body)`，让路由的静默漂移响亮地失败：路径参数的百分号
 * 编码、列表过滤的查询串投影，以及路径参数必须从线上载荷里剥离时的 body 投影。
 * 其余资源分组见同目录的其它分片。
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

  it('projectsList → GET /api/projects', async () => {
    await invokeCmd(COMMANDS.projectsList);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/projects');
    assert.equal(c.body, undefined);
  });

  it('projectsGet → GET /api/projects/{id} with url-encoded id', async () => {
    await invokeCmd(COMMANDS.projectsGet, { id: 'a/b c' });
    const c = last();
    assert.equal(c.method, 'GET');
    // '/' 与 ' ' 必须百分号编码；不该有任何裸字符漏出去。
    assert.equal(c.url, '/api/projects/a%2Fb%20c');
  });

  it('projectsUpdate → PATCH /api/projects/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.projectsUpdate, {
      id: 'p1',
      patch: { name: 'New name' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/projects/p1');
    // `id` 绝不能漏进 body —— Rust handler 把 body 反序列化成 `ProjectPatch`，多出来的
    // `id` 字段轻则被静默忽略。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'New name' });
  });

  it('projectsDelete → DELETE /api/projects/{id}, no body', async () => {
    await invokeCmd(COMMANDS.projectsDelete, { id: 'p1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/projects/p1');
    assert.equal(c.body, undefined);
  });

  it('nodesList → GET /api/nodes with rootId in query string', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/nodes?rootId=pX');
    assert.equal(c.body, undefined);
  });

  it('nodesList (no filter) → GET /api/nodes (no query string)', async () => {
    // 列表命令在渲染层有时不带参数调用（`invoke<ResourceNode[]>(COMMANDS.nodesList)`）；
    // URL 不该因此多出一个尾随的 `?`。
    await invokeCmd(COMMANDS.nodesList);
    const c = last();
    assert.equal(c.url, '/api/nodes');
  });

  it('nodesList → projects type and query onto the same canonical route', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX', type: 'endpoint', query: 'login' });
    const c = last();
    // `buildQuery` 会给键排序，所以线上顺序是字母序（`q` < `rootId` < `type`），不是
    // 调用方的参数顺序。
    assert.equal(c.url, '/api/nodes?q=login&rootId=pX&type=endpoint');
  });

  it('nodesList → 三个过滤条件落到同一条路由，query 也会被编码', async () => {
    await invokeCmd(COMMANDS.nodesList, { rootId: 'pX', type: 'endpoint', query: 'a b&c' });
    const c = last();
    assert.equal(c.url, '/api/nodes?q=a%20b%26c&rootId=pX&type=endpoint');
  });

  it('nodesGet → GET /api/nodes/{id} with the id percent-encoded', async () => {
    await invokeCmd(COMMANDS.nodesGet, { id: 'a/b' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/nodes/a%2Fb');
    assert.equal(c.body, undefined);
  });

  it('nodesCreate → POST /api/nodes with a FLAT body (NodeCreateBody flattens the node)', async () => {
    await invokeCmd(COMMANDS.nodesCreate, {
      id: 'n1',
      rootId: 'p1',
      parentId: 'c1',
      kind: 'endpoint',
      name: 'Login',
      description: 'auth',
    });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes');
    // 节点自身的字段**就是** body。套一层 `{ node: {…} }` 会无法反序列进
    // `NodeCreateBody` 的 `#[serde(flatten)]`。`sortOrder` 在服务端没有默认值，所以
    // 缺省时必须变成 `0` 而不是 `undefined`（后者会被 `JSON.stringify` 丢掉）。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      id: 'n1',
      rootId: 'p1',
      parentId: 'c1',
      kind: 'endpoint',
      name: 'Login',
      description: 'auth',
      sortOrder: 0,
    });
  });

  it('nodesCreate → defaults parentId to null and stamps detail.nodeId from the id', async () => {
    await invokeCmd(COMMANDS.nodesCreate, {
      id: 'n1',
      rootId: 'p1',
      kind: 'module',
      name: 'Core',
      sortOrder: 3,
      detail: { method: 'POST', url: '/login' },
    });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    // module 的父级是项目，用 `null` 表达。
    assert.equal(body.parentId, null);
    assert.equal(body.sortOrder, 3);
    // 线上 `endpoint_details.node_id` 是必填；客户端本来就知道它，所以由投影注入，
    // 而不是让调用方重复一遍 id。
    assert.deepEqual(body.detail, { method: 'POST', url: '/login', nodeId: 'n1' });
  });

  it('nodesUpdate → PATCH /api/nodes/{id} with body = patch only', async () => {
    await invokeCmd(COMMANDS.nodesUpdate, { id: 'n1', patch: { name: 'Renamed' } });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/nodes/n1');
    // `id` 绝不能漏进 body —— Rust handler 把 body 反序列化成 `NodePatch`。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { name: 'Renamed' });
  });

  it('nodesDelete → DELETE /api/nodes/{id}, no body', async () => {
    await invokeCmd(COMMANDS.nodesDelete, { id: 'n1' });
    const c = last();
    assert.equal(c.method, 'DELETE');
    assert.equal(c.url, '/api/nodes/n1');
    assert.equal(c.body, undefined);
  });

  it('nodesMove → POST /api/nodes/move with a flat body (camelCase match)', async () => {
    await invokeCmd(COMMANDS.nodesMove, { nodeIds: ['m1', 'c1'], parentId: 'p1', index: 2 });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes/move');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), {
      nodeIds: ['m1', 'c1'],
      parentId: 'p1',
      index: 2,
    });
  });

  it('nodesMove → a top-level drop sends parentId: null, not undefined', async () => {
    // 落到项目根是一个真实的移动目标，而 `null` 正是 `move_many` 读作「改挂到项目」的
    // 值。留成 `undefined` 会让 `JSON.stringify` 丢掉这个键，服务端就会保留旧父级。
    await invokeCmd(COMMANDS.nodesMove, { nodeIds: ['c1'], index: 0 });
    const c = last();
    const body = JSON.parse(c.body ?? 'null');
    assert.equal(body.parentId, null);
    assert.deepEqual(body.nodeIds, ['c1']);
  });

  it('nodesDuplicate → POST /api/nodes/{id}/duplicate, carrying newId', async () => {
    // `newId` 是副本落库所用的 id —— SPA 乐观地铸出它，并需要服务端保留，否则它已经
    // 打开的那个页签会指向一条谁都不拥有的行。
    await invokeCmd(COMMANDS.nodesDuplicate, { id: 'n1', newId: 'copy-1' });
    const c = last();
    assert.equal(c.method, 'POST');
    assert.equal(c.url, '/api/nodes/n1/duplicate');
    // path 吃掉了 `id`；body 投影器只交回 `newId`，好让线上载荷不带 Rust handler 会
    // 静默丢弃的 `{ id }` 字段。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { newId: 'copy-1' });
  });

  it('nodesDuplicate → no newId means "let the server mint one"', async () => {
    // 没有本地副本需要保持一致的调用方（脚本、更老的客户端）会省略它，而 handler 里的
    // `null` 意思正是如此——不是「拒绝这个请求」。
    await invokeCmd(COMMANDS.nodesDuplicate, { id: 'n1' });
    const c = last();
    assert.equal(c.url, '/api/nodes/n1/duplicate');
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { newId: null });
  });

  it('endpointDetailsList → GET /api/endpoint-details with no body', async () => {
    await invokeCmd(COMMANDS.endpointDetailsList);
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/endpoint-details');
    assert.equal(c.body, undefined);
  });

  it('endpointDetailGet → GET /api/nodes/{nodeId}/endpoint-detail', async () => {
    await invokeCmd(COMMANDS.endpointDetailGet, { nodeId: 'n1' });
    const c = last();
    assert.equal(c.method, 'GET');
    assert.equal(c.url, '/api/nodes/n1/endpoint-detail');
    assert.equal(c.body, undefined);
  });

  it('endpointDetailUpdate → PATCH /api/nodes/{nodeId}/endpoint-detail with body = patch only', async () => {
    await invokeCmd(COMMANDS.endpointDetailUpdate, {
      nodeId: 'n1',
      patch: { method: 'POST' },
    });
    const c = last();
    assert.equal(c.method, 'PATCH');
    assert.equal(c.url, '/api/nodes/n1/endpoint-detail');
    // 节点 id 走 path；在 body 里重复它会被 `EndpointDetailPatch` 静默丢弃。
    assert.deepEqual(JSON.parse(c.body ?? 'null'), { method: 'POST' });
  });
});
