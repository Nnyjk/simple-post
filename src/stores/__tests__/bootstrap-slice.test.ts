/**
 * Bootstrap 契约测试：锁定无过滤参数的聚合加载，以及失败时的错误状态。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { clearIpcMock, installBrowserShim, installIpcMock } from './_browser-shim';
import { createBootstrapSlice, type BootstrapSlice } from '../bootstrap-slice';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';

type CapturedCall = { cmd: string; args: Record<string, unknown> };

function makeBootstrapHarness() {
  const bag: Record<string, unknown> & Partial<BootstrapSlice> = {
    projects: [],
    nodes: [],
    endpointDetails: {},
    environments: [],
    loaded: false,
    activeProjectId: '',
    activeEnvironmentId: '',
    activeEndpointId: null,
    loading: false,
    bootstrapError: null,
    dbRecreatedAt: null,
    openTab: () => undefined,
    setLastResponse: () => undefined,
  };
  const set = (partial: unknown) => {
    const next =
      typeof partial === 'function' ? (partial as (state: typeof bag) => unknown)(bag) : partial;
    if (next && typeof next === 'object') Object.assign(bag, next);
  };
  const get = () => bag;
  Object.assign(bag, createBootstrapSlice(set as never, get as never, {} as never));
  return bag as Record<string, unknown> & BootstrapSlice;
}

const project: Project = {
  id: 'p1',
  name: '项目',
  baseUrlDefinitions: [],
  defaultBaseUrlDefinitionId: null,
  color: '#000000',
  createdAt: 1,
  updatedAt: 1,
};
const moduleNode: ResourceNode = {
  id: 'm1',
  rootId: 'p1',
  parentId: null,
  type: 'module',
  name: '模块',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};
const collectionNode: ResourceNode = {
  id: 'c1',
  rootId: 'p1',
  parentId: 'm1',
  type: 'collection',
  name: '集合',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};
const endpointNode: ResourceNode = {
  id: 'e1',
  rootId: 'p1',
  parentId: 'c1',
  type: 'endpoint',
  name: '接口',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};
const environment: Environment = {
  id: 'env1',
  projectId: 'p1',
  name: '开发',
  baseUrls: {},
  variables: {},
  isActive: true,
};
const endpointDetail: EndpointDetail = {
  nodeId: 'e1',
  method: 'GET',
  url: '/health',
  params: [],
  headers: [],
  body: { mode: 'none', content: '' },
  auth: { type: 'none' },
  tags: [],
};

describe('bootstrap-slice 聚合加载契约', () => {
  let originalError: typeof console.error;

  beforeEach(() => {
    installBrowserShim();
    originalError = console.error;
    console.error = () => undefined;
  });

  afterEach(() => {
    console.error = originalError;
    clearIpcMock();
  });

  it('Tauri 形状以空参数加载全部列表并一次性提交状态', async () => {
    const bag = makeBootstrapHarness();
    const calls: CapturedCall[] = [];
    const responses: Record<string, unknown> = {
      projects_list: [project],
      // One flat tree replaces the old modules/collections/endpoints trio.
      nodes_list: [moduleNode, collectionNode, endpointNode],
      endpoint_details_list: [endpointDetail],
      environments_list: [environment],
    };
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args });
      return responses[cmd];
    });

    await bag.bootstrap();

    // `Promise.all` starts every call in order, so the captured list is
    // deterministic even though the resolutions interleave.
    assert.deepEqual(calls, [
      { cmd: 'projects_list', args: {} },
      { cmd: 'nodes_list', args: {} },
      { cmd: 'endpoint_details_list', args: {} },
      { cmd: 'environments_list', args: {} },
      // Boot-time notice check, issued after the lists resolve and
      // tolerant of failure — see the reset-announcement tests below.
      { cmd: 'db_recreated_at', args: {} },
    ]);
    assert.equal(bag.loaded, true);
    assert.equal(bag.loading, false);
    assert.equal(bag.bootstrapError, null);
    assert.equal(bag.dbRecreatedAt, null);
    assert.deepEqual(bag.projects, [project]);
    assert.deepEqual(bag.nodes, [moduleNode, collectionNode, endpointNode]);
    // The detail list is keyed by node id, not carried as an array.
    assert.deepEqual(bag.endpointDetails, { e1: endpointDetail });
    assert.deepEqual(bag.environments, [environment]);
    assert.equal(bag.activeProjectId, 'p1');
    assert.equal(bag.activeEnvironmentId, 'env1');
  });

  it('首个接口只在首个项目内挑选（按 rootId，而不是全局第一个）', async () => {
    const bag = makeBootstrapHarness();
    const otherProject: Project = { ...project, id: 'p2', name: '别的项目' };
    // A whole-workspace list would hand back this node first, and the
    // user would land in a project the tree never showed them.
    const otherEndpoint: ResourceNode = { ...endpointNode, id: 'e0', rootId: 'p2' };
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project, otherProject];
      if (cmd === 'nodes_list') return [otherEndpoint, endpointNode];
      if (cmd === 'endpoint_details_list') return [endpointDetail];
      if (cmd === 'environments_list') return [];
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.activeProjectId, 'p1');
    assert.equal(bag.activeEndpointId, 'e1', 'not the other project’s endpoint');
  });

  it('读取 endpoint detail 失败时软降级为 {}，不影响其余数据', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'nodes_list') return [endpointNode];
      // The tree still renders; only the HTTP tester comes up blank.
      if (cmd === 'endpoint_details_list') throw new Error('detail store offline');
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.loaded, true);
    assert.equal(bag.bootstrapError, null);
    assert.deepEqual(bag.endpointDetails, {});
    assert.deepEqual(bag.nodes, [endpointNode]);
  });

  it('后端报告 schema 重建时把时间戳交给 UI 展示', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'db_recreated_at') return 1789623874000;
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.loaded, true);
    assert.equal(bag.bootstrapError, null, 'a reset workspace loaded fine');
    assert.equal(bag.dbRecreatedAt, 1789623874000);
  });

  it('读取重建时间戳失败不影响加载结果', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      // Losing the data because we could not read a notice about
      // losing the data would be absurd.
      if (cmd === 'db_recreated_at') throw new Error('recreated_at unavailable');
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.loaded, true, 'the lists still loaded');
    assert.equal(bag.bootstrapError, null);
    assert.equal(bag.dbRecreatedAt, null);
  });

  it('用户可以关闭重建提示而不影响已加载数据', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => (cmd === 'db_recreated_at' ? 1789623874000 : []));
    await bag.bootstrap();
    assert.equal(bag.dbRecreatedAt, 1789623874000);

    bag.dismissDbRecreated();

    assert.equal(bag.dbRecreatedAt, null);
    assert.equal(bag.loaded, true, 'dismissing is not a reload');
  });

  it('任一必选列表失败时保留错误且不伪装成成功空数据', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'environments_list') throw new Error('environment store offline');
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.loaded, false);
    assert.equal(bag.loading, false);
    assert.equal(bag.bootstrapError, 'environment store offline');
    assert.deepEqual(bag.projects, []);
    assert.deepEqual(bag.environments, []);
  });
});
