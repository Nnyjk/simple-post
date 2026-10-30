/**
 * `crud-slice` 的 `addNode` id 归属：新建节点的 id 以**服务端返回值**为准。
 *
 * 本地乐观铸出的 id 只是占位——后端可以另起一个，而 detail 行必须挂在它返回的那一个
 * 上，否则接口配置会挂到一条谁都不拥有的行。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import { makeCrudHarness } from './crud-slice-harness';
import type { ResourceNode } from '@/types/domain';
// ============================================================
// addNode —— detail 映射必须按服务端返回的 id 作键
// ============================================================

describe('crud-slice: addNode 的 id 以服务端返回值为准', () => {
  /** 一个后端会交还的节点，带着它自己的 id。 */
  function serverNode(id: string, overrides: Partial<ResourceNode> = {}): ResourceNode {
    return {
      id,
      rootId: 'p1',
      parentId: 'm1',
      kind: 'endpoint',
      name: 'Login',
      sortOrder: 0,
      createdAt: 0,
      updatedAt: 0,
      ...overrides,
    };
  }

  it('后端另起 id 时，detail 挂在它返回的 id 上而不是本地 id', async () => {
    const harness = makeCrudHarness();
    // Tauri 的 `nodes_create` 命令自己铸一个 `uid()` 并丢掉调用方发的 id，
    // 所以这就是桌面端的形状。
    installIpcMock(async (cmd) => {
      if (cmd === 'nodes_create') return serverNode('server-id');
      throw new Error('unexpected cmd: ' + cmd);
    });

    const returned = await harness.slice.addNode({
      parentId: 'm1',
      kind: 'endpoint',
      name: 'Login',
      detail: { url: '' },
    });

    assert.equal(returned, 'server-id', '返回的 id 必须是服务端那个');
    assert.equal(harness.bag.nodes.at(-1)?.id, 'server-id');
    // 这个守卫防的 bug：按本地 id 作键会把 detail 写在一个没有任何节点持有的键之下，
    // 于是 `EndpointList` 跳过这一行，工作区把标签解析成它的"已删除"描述符。
    assert.ok(
      harness.bag.endpointDetails['server-id'],
      'detail 必须按服务端 id 落位，否则树里那一行永远渲染不出来',
    );
    assert.equal(
      Object.keys(harness.bag.endpointDetails).length,
      1,
      '不应在本地 id 下留下一条孤儿 detail',
    );
    assert.equal(harness.bag.endpointDetails['server-id'].nodeId, 'server-id');
  });

  it('后端沿用调用方 id 时行为一致', async () => {
    const harness = makeCrudHarness();
    // REST 的形状：`ensure_server_id` 保留调用方提供的 id。
    installIpcMock(async (cmd, args) => {
      if (cmd === 'nodes_create') return serverNode(String(args?.id));
      throw new Error('unexpected cmd: ' + cmd);
    });

    const returned = await harness.slice.addNode({
      parentId: 'm1',
      kind: 'endpoint',
      name: 'Login',
      detail: { url: '' },
    });

    assert.equal(returned, harness.bag.nodes.at(-1)?.id);
    assert.ok(harness.bag.endpointDetails[returned!], 'detail 必须挂在返回的 id 上');
    assert.equal(Object.keys(harness.bag.endpointDetails).length, 1);
  });

  it('非 endpoint 不写 detail 行', async () => {
    const harness = makeCrudHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'nodes_create') return serverNode('m-server', { kind: 'module', parentId: null });
      throw new Error('unexpected cmd: ' + cmd);
    });

    await harness.slice.addNode({ rootId: 'p1', kind: 'module', name: 'Core' });

    assert.deepEqual(
      harness.bag.endpointDetails,
      {},
      '给模块写一条空 detail 会让 Object.keys(endpointDetails) 说谎',
    );
  });
});
