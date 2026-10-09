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
  kind: 'module',
  name: '模块',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};
const collectionNode: ResourceNode = {
  id: 'c1',
  rootId: 'p1',
  parentId: 'm1',
  kind: 'collection',
  name: '集合',
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
};
const endpointNode: ResourceNode = {
  id: 'e1',
  rootId: 'p1',
  parentId: 'c1',
  kind: 'endpoint',
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
  auth: { kind: 'none' },
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
    ]);
    assert.equal(bag.loaded, true);
    assert.equal(bag.loading, false);
    assert.equal(bag.bootstrapError, null);
    assert.deepEqual(bag.projects, [project]);
    assert.deepEqual(bag.nodes, [moduleNode, collectionNode, endpointNode]);
    // The detail list is keyed by node id, not carried as an array.
    assert.deepEqual(bag.endpointDetails, { e1: endpointDetail });
    assert.deepEqual(bag.environments, [environment]);
    // Nothing is selected at boot — see the next test.
    assert.equal(bag.activeProjectId, '');
    assert.equal(bag.activeEnvironmentId, '');
    assert.equal(bag.activeEndpointId, null);
  });

  it('启动不预选任何项目或接口，把选择权留给欢迎页', async () => {
    const bag = makeBootstrapHarness();
    const otherProject: Project = { ...project, id: 'p2', name: '别的项目' };
    // Both projects are populated. An older bootstrap picked
    // `projects[0]` and its first endpoint, so a user with several
    // projects never saw the picker and never saw the project they were
    // dropped into either.
    const otherEndpoint: ResourceNode = { ...endpointNode, id: 'e0', rootId: 'p2' };
    const opened: { kind: string; id: string }[] = [];
    bag.openTab = (kind: string, id: string) => opened.push({ kind, id });
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project, otherProject];
      if (cmd === 'nodes_list') return [otherEndpoint, endpointNode];
      if (cmd === 'endpoint_details_list') return [endpointDetail];
      if (cmd === 'environments_list') return [environment];
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.activeProjectId, '', 'no project is entered on load');
    assert.equal(bag.activeEnvironmentId, '', 'and therefore no environment');
    assert.equal(bag.activeEndpointId, null);
    assert.deepEqual(opened, [], 'no tab is opened behind the welcome screen');
  });

  it('openProject 选中项目并激活它的标签', async () => {
    const bag = makeBootstrapHarness();
    const opened: { kind: string; id: string }[] = [];
    const selected: string[] = [];
    bag.openTab = (kind: string, id: string) => opened.push({ kind, id });
    bag.setActiveProject = (id: string) => selected.push(id);
    installIpcMock(async (cmd) => (cmd === 'projects_list' ? [project] : []));

    bag.openProject('p1');

    // Both halves, in this order: the tree only re-scopes once the
    // project is the active one, and the right pane must not be left
    // showing the previous project's entity.
    assert.deepEqual(selected, ['p1']);
    assert.deepEqual(opened, [{ kind: 'project', id: 'p1' }]);
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
