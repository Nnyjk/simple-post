/**
 * `duplicateEndpoint` —— 乐观复制的 id 必须就是持久化的 id。
 *
 * 契约：调用方在前端 `uid()` 生成 id 写进 store 并**同步**返回之后，就立刻用它
 * 开 tab、进入重命名；所以后端必须采纳这个 id。若后端自己再发一个，回调里把
 * 乐观行替换成后端那份，`openTabs` / `activeEndpointId` / `setEditing` 就全都
 * 指向了一个谁也不拥有的行，`useTabDescriptors` 查不到便走 `missingDescriptor`
 * ——树里多出一行正常副本，标签标题却是「（接口已删除）」。
 *
 * 这里跑的是**真实组合 store**（`useAppStore`）而不是手搓 harness：坏的
 * 恰恰是切片之间的协作（crud 写 id，ui-tabs 用它开 tab），单独测一个切片
 * 看不到这个洞。
 *
 * 运行：
 *   npx tsx --test src/stores/__tests__/crud-duplicate.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { clearIpcMock, installBrowserShim, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import type { EndpointDetail, ResourceNode } from '@/types/domain';

type CapturedInvoke = { cmd: string; args: Record<string, unknown> };

const module1: ResourceNode = {
  id: 'm1',
  rootId: 'p1',
  parentId: null,
  kind: 'module',
  name: '用户模块',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};

const collection1: ResourceNode = {
  id: 'c1',
  rootId: 'p1',
  parentId: 'm1',
  kind: 'collection',
  name: '用户',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};

const endpoint1: ResourceNode = {
  id: 'e1',
  rootId: 'p1',
  parentId: 'c1',
  kind: 'endpoint',
  name: 'Fetch one',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};

function detail(nodeId: string): EndpointDetail {
  return {
    nodeId,
    method: 'GET',
    url: '/users',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags: [],
  };
}

/** 让后台那次 fire-and-forget invoke 走完。 */
function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  installBrowserShim();
  useAppStore.setState({
    activeProjectId: 'p1',
    nodes: [module1, collection1, endpoint1],
    endpointDetails: { e1: detail('e1') },
    openTabs: [],
    activeTabId: null,
    activeEndpointId: null,
  });
});

afterEach(() => {
  clearIpcMock();
});

describe('duplicateEndpoint：id 与 name 两端一致', () => {
  it('副本在同一个 id 下落地，tab 能查到那一行，detail 也跟着走', async () => {
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      // 后端沿用调用方的 id。
      return { ...endpoint1, id: String(args?.newId), name: 'Fetch one (副本)' };
    });

    const newId = useAppStore.getState().duplicateEndpoint('e1');
    assert.ok(newId, '副本 id 必须同步返回，调用方要靠它开 tab');
    assert.notEqual(newId, 'e1');

    // 调用方（useEditMenu）拿到 id 之后的真实动作。
    useAppStore.getState().setActiveEndpoint(newId);
    await flush();

    const state = useAppStore.getState();
    const copy = state.nodes.find((n) => n.id === newId);
    assert.ok(copy, 'nodes 里必须有这一行 —— tab 描述符就是按它渲染的');
    assert.equal(copy.name, 'Fetch one (副本)');
    assert.equal(copy.parentId, 'c1', '副本紧挨着原接口');
    assert.equal(copy.sortOrder, 1);

    assert.ok(
      state.endpointDetails[newId],
      'detail 必须挂在同一个 id 下，否则 HTTP tester 打不开这一行',
    );
    assert.equal(state.endpointDetails[newId].nodeId, newId);
    assert.equal(state.endpointDetails[newId].url, '/users');

    // 这就是用户看到「（接口已删除）」的那一步：激活的 tab 必须能查到行。
    assert.equal(state.activeEndpointId, newId);
    assert.equal(state.activeTabId, newId);
    assert.deepEqual(
      state.openTabs.map((t) => t.id),
      [newId],
    );
    assert.ok(
      state.nodes.some((n) => n.id === state.activeTabId),
      'activeTabId 必须在 nodes 里有一行对应，否则走 missingDescriptor',
    );

    assert.deepEqual(calls, [{ cmd: 'nodes_duplicate', args: { id: 'e1', newId } }]);
  });

  it('后端返回后不覆盖乐观行 —— 用户已经改过的副本名不能被回滚', async () => {
    installIpcMock(async (cmd, args) => {
      if (cmd === 'nodes_duplicate') {
        // 服务端返回它写入的那一行。id 相同、内容过时：修复前的 `.then()`
        // 会拿它把乐观行整个换掉，连同用户在这期间敲进去的改名一起丢掉。
        return { ...endpoint1, id: String(args?.newId), name: 'Fetch one (副本)' };
      }
      if (cmd === 'nodes_update') return null;
      throw new Error('unexpected cmd: ' + cmd);
    });

    const newId = useAppStore.getState().duplicateEndpoint('e1')!;
    // 复制后立刻重命名 —— `useEditMenu` 打开的就是行内编辑框。
    await useAppStore.getState().updateNode(newId, { name: 'Fetch one（改）' });
    await flush();

    const state = useAppStore.getState();
    assert.equal(state.nodes.find((n) => n.id === newId)?.name, 'Fetch one（改）');
    assert.equal(state.nodes.filter((n) => n.id === newId).length, 1, '成功回调不应再造一行');
  });

  it('落库失败时撤掉乐观副本，原接口不受影响', async () => {
    installIpcMock(async () => {
      throw new Error('sqlite: disk I/O error');
    });

    const newId = useAppStore.getState().duplicateEndpoint('e1')!;
    await flush();

    const state = useAppStore.getState();
    assert.equal(
      state.nodes.some((n) => n.id === newId),
      false,
      '失败后不能留下幽灵行',
    );
    assert.equal(state.endpointDetails[newId], undefined, 'detail 也要一起撤掉');
    assert.ok(
      state.nodes.some((n) => n.id === 'e1'),
      '原接口还在',
    );
    assert.ok(state.endpointDetails['e1'], '原接口的 detail 还在');
  });
});
