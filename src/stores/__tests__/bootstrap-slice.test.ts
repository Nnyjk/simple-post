/**
 * Bootstrap 契约测试：锁定无过滤参数的聚合加载，以及失败时的错误状态。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { clearIpcMock, installBrowserShim, installIpcMock } from './_browser-shim';
import { createBootstrapSlice, type BootstrapSlice } from '../bootstrap-slice';
import type { Collection, Environment, Endpoint, Module, Project } from '@/types/domain';

type CapturedCall = { cmd: string; args: Record<string, unknown> };

function makeBootstrapHarness() {
  const bag: Record<string, unknown> & Partial<BootstrapSlice> = {
    projects: [],
    modules: [],
    collections: [],
    endpoints: [],
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
const module: Module = {
  id: 'm1',
  projectId: 'p1',
  name: '模块',
  sortOrder: 0,
};
const collection: Collection = {
  id: 'c1',
  moduleId: 'm1',
  parentCollectionId: null,
  name: '集合',
  sortOrder: 0,
};
const environment: Environment = {
  id: 'env1',
  projectId: 'p1',
  name: '开发',
  baseUrls: {},
  variables: {},
  isActive: true,
};
const endpoints: Endpoint[] = [];

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
      modules_list: [module],
      collections_list: [collection],
      endpoints_list: endpoints,
      environments_list: [environment],
    };
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args });
      return responses[cmd];
    });

    await bag.bootstrap();

    assert.deepEqual(calls, [
      { cmd: 'projects_list', args: {} },
      { cmd: 'modules_list', args: {} },
      { cmd: 'collections_list', args: {} },
      { cmd: 'endpoints_list', args: {} },
      { cmd: 'environments_list', args: {} },
    ]);
    assert.equal(bag.loaded, true);
    assert.equal(bag.loading, false);
    assert.equal(bag.bootstrapError, null);
    assert.deepEqual(bag.projects, [project]);
    assert.deepEqual(bag.modules, [module]);
    assert.deepEqual(bag.collections, [collection]);
    assert.deepEqual(bag.environments, [environment]);
    assert.equal(bag.activeProjectId, 'p1');
    assert.equal(bag.activeEnvironmentId, 'env1');
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
