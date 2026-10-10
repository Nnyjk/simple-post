/**
 * Bootstrap 契约测试：锁定**两层**加载的时序，以及失败时的错误状态。
 *
 * 分层是这一版的核心，所以测试盯的不是"发了哪几个命令"，而是"欢迎页
 * 在哪一刻可以画"：`loaded` 必须在只有 `projects_list` 落地时就为 true，
 * 而 `nodesLoaded` 要等到树那一组读完。后者若和前者一起翻 true，启动画面
 * 就又回到了"等最慢的那个查询"。
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
    nodesLoaded: false,
    nodesLoading: false,
    activeProjectId: '',
    activeEnvironmentId: '',
    activeEndpointId: null,
    loading: false,
    bootstrapError: null,
    nodesError: null,
    openTab: () => undefined,
    // Entering/leaving a project now hands the tab strip to `ui-tabs`
    // rather than opening a tab from here. Stubbed for the same reason
    // `openTab` is: the swap itself is `ui-tabs`' contract and is covered
    // by `project-tabs.test.ts`; what these tests care about is *when*
    // `bootstrap-slice` asks for it.
    switchProjectTabs: () => undefined,
    closeTab: () => undefined,
    setLastResponse: () => undefined,
    loadWorkspace: async () => undefined,
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

  it('第一层只读 projects：欢迎页不必等树那一组', async () => {
    const bag = makeBootstrapHarness();
    const calls: CapturedCall[] = [];
    const responses: Record<string, unknown> = {
      projects_list: [project],
      // One flat tree replaces the old modules/collections/endpoints trio.
      nodes_list: [moduleNode, collectionNode, endpointNode],
      endpoint_details_list: [endpointDetail],
      environments_list: [environment],
    };
    // Gate the tree layer so we can observe the store mid-bootstrap. This
    // is the whole point of the split: `loaded` must already be true
    // while these are still pending.
    let releaseTree: () => void = () => undefined;
    const treeGate = new Promise<void>((resolve) => {
      releaseTree = resolve;
    });
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd !== 'projects_list') await treeGate;
      return responses[cmd];
    });

    const booting = bag.bootstrap();
    // Wait for layer 1 to settle without releasing the tree gate. A
    // macrotask tick drains every pending microtask, which is exactly
    // the boundary layer 1's `set()` lands on — counting
    // `Promise.resolve()`s would be a guess about how many awaits
    // `invokeCmd` happens to contain.
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(bag.loaded, true, 'welcome screen can paint on the project list alone');
    assert.equal(bag.nodesLoaded, false, 'tree has not landed yet');
    assert.deepEqual(bag.projects, [project]);
    // Layer 2 *starts* right away — it is only un-awaited that keeps it
    // off the critical path. So the assertion is not "these commands were
    // not issued" but "they have not resolved", which is what `loaded`
    // being true alongside `nodesLoaded` false already proves.
    assert.deepEqual(
      calls.map((c) => c.cmd),
      ['projects_list', 'nodes_list', 'endpoint_details_list', 'environments_list'],
      'layer 2 is dispatched, just never awaited by bootstrap',
    );

    releaseTree();
    await booting;
    // `bootstrap()` returns as soon as layer 1 lands — that is the whole
    // point of the split. Awaiting `loadWorkspace()` instead joins the
    // pass bootstrap already started, which is only possible because an
    // in-flight load hands back its pending promise instead of resolving
    // immediately.
    await bag.loadWorkspace();

    assert.equal(bag.nodesLoaded, true);
    assert.equal(bag.nodesLoading, false);
    assert.equal(bag.bootstrapError, null);
    assert.equal(bag.nodesError, null);
    assert.deepEqual(calls.slice(1), [
      { cmd: 'nodes_list', args: {} },
      { cmd: 'endpoint_details_list', args: {} },
      { cmd: 'environments_list', args: {} },
    ]);
    assert.deepEqual(bag.nodes, [moduleNode, collectionNode, endpointNode]);
    // The detail list is keyed by node id, not carried as an array.
    assert.deepEqual(bag.endpointDetails, { e1: endpointDetail });
    assert.deepEqual(bag.environments, [environment]);
    // Nothing is selected at boot — see the next test.
    assert.equal(bag.activeProjectId, '');
    assert.equal(bag.activeEnvironmentId, '');
    assert.equal(bag.activeEndpointId, null);
  });

  it('loadWorkspace 幂等：重复调用不会重复拉取', async () => {
    const bag = makeBootstrapHarness();
    const calls: string[] = [];
    installIpcMock(async (cmd) => {
      calls.push(cmd);
      return [];
    });

    await bag.loadWorkspace();
    await bag.loadWorkspace();
    await bag.loadWorkspace();

    assert.deepEqual(
      calls,
      ['nodes_list', 'endpoint_details_list', 'environments_list'],
      'a second call after the first is a no-op',
    );
  });

  it('并发调用 loadWorkspace 不会并发发起第二轮 IPC', async () => {
    const bag = makeBootstrapHarness();
    const calls: string[] = [];
    let releaseTree: () => void = () => undefined;
    const treeGate = new Promise<void>((resolve) => {
      releaseTree = resolve;
    });
    installIpcMock(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'nodes_list') await treeGate;
      return [];
    });

    // `bootstrap`, `openProject` and the retry button all reach for this
    // without coordinating — the in-flight guard is what keeps them from
    // stacking three copies of the same query.
    const first = bag.loadWorkspace();
    const second = bag.loadWorkspace();
    assert.equal(bag.nodesLoading, true);

    releaseTree();
    await Promise.all([first, second]);

    assert.equal(calls.filter((c) => c === 'nodes_list').length, 1);
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

  it('openProject 选中项目、切换到它的标签区，并顺带触发树加载', async () => {
    const bag = makeBootstrapHarness();
    const opened: { kind: string; id: string }[] = [];
    const selected: string[] = [];
    const switched: string[] = [];
    let workspaceLoads = 0;
    bag.openTab = (kind: string, id: string) => opened.push({ kind, id });
    bag.switchProjectTabs = (id: string) => switched.push(id);
    bag.setActiveProject = (id: string) => selected.push(id);
    // A user who clicks a project card before the background pass lands
    // must still get a tree, so entering a project re-requests it.
    bag.loadWorkspace = async () => {
      workspaceLoads += 1;
    };
    installIpcMock(async (cmd) => (cmd === 'projects_list' ? [project] : []));

    bag.openProject('p1');

    // The switch runs *before* the selection moves, and that order is the
    // point: setting aside the outgoing project's tabs means asking which
    // project owns each one, which is only answerable while the outgoing
    // project is still the one in front.
    assert.deepEqual(switched, ['p1']);
    assert.deepEqual(selected, ['p1']);
    assert.deepEqual(
      opened,
      [],
      'the strip swap owns putting the project tab on screen now, not this slice',
    );
    assert.equal(workspaceLoads, 1, 'entering a project pulls the tree layer');
  });

  it('leaveProject 回到欢迎页，但保留已加载的树与标签', async () => {
    const bag = makeBootstrapHarness();
    const closed: string[] = [];
    bag.closeTab = (id: string) => closed.push(id);
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      if (cmd === 'nodes_list') return [moduleNode, collectionNode, endpointNode];
      if (cmd === 'endpoint_details_list') return [endpointDetail];
      if (cmd === 'environments_list') return [environment];
      return [];
    });

    await bag.bootstrap();
    await bag.loadWorkspace();
    bag.openProject('p1');
    assert.equal(bag.activeProjectId, 'p1');

    bag.leaveProject();

    // `App.tsx` switches surfaces on `activeProjectId === ''`, so clearing
    // it is the whole of "go back to the welcome page".
    assert.equal(bag.activeProjectId, '');
    assert.equal(
      bag.activeEnvironmentId,
      '',
      'an env from the abandoned project is meaningless here',
    );
    assert.equal(bag.activeEndpointId, null);
    // Re-entering must not refetch: the tree is per-project state the user
    // will very likely want back on the next click.
    assert.equal(bag.nodesLoaded, true);
    assert.deepEqual(bag.nodes, [moduleNode, collectionNode, endpointNode]);
    assert.deepEqual(closed, [], 'leaving is not closing tabs');
  });

  it('leaveProject 在欢迎页上是 no-op', () => {
    const bag = makeBootstrapHarness();
    bag.activeProjectId = '';
    bag.leaveProject();
    assert.equal(bag.activeProjectId, '');
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
    await bag.loadWorkspace();

    assert.equal(bag.loaded, true);
    assert.equal(bag.nodesLoaded, true);
    assert.equal(bag.bootstrapError, null);
    assert.equal(bag.nodesError, null);
    assert.deepEqual(bag.endpointDetails, {});
    assert.deepEqual(bag.nodes, [endpointNode]);
  });

  it('第一层失败时不伪装成成功空数据', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') throw new Error('project store offline');
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.loaded, false);
    assert.equal(bag.loading, false);
    assert.equal(bag.bootstrapError, 'project store offline');
    assert.deepEqual(bag.projects, []);
  });

  it('第二层失败时欢迎页仍然可用：错误落在 nodesError，不卡住启动', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      if (cmd === 'environments_list') throw new Error('environment store offline');
      return [];
    });

    await bag.bootstrap();
    await bag.loadWorkspace();

    // The distinguishing assertion of the split: layer 1 succeeding is
    // enough for the welcome screen, so `loaded` is true even though the
    // tree layer failed. Merging these two flags back together would
    // either strand the boot splash or hide a real tree failure.
    assert.equal(bag.loaded, true, 'welcome screen still paints');
    assert.deepEqual(bag.projects, [project]);
    assert.equal(bag.nodesLoaded, false);
    assert.equal(bag.nodesLoading, false);
    assert.equal(bag.bootstrapError, null, 'not reported as a startup failure');
    assert.equal(bag.nodesError, 'environment store offline');
    assert.deepEqual(bag.environments, []);
  });

  it('第二层在项目已进入后才落地时，补选该项目的 active 环境', async () => {
    const bag = makeBootstrapHarness();
    let releaseTree: () => void = () => undefined;
    const treeGate = new Promise<void>((resolve) => {
      releaseTree = resolve;
    });
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      if (cmd === 'environments_list') {
        // Arrives after the user already clicked the card.
        await treeGate;
        return [environment];
      }
      return [];
    });

    const booting = bag.bootstrap();
    await new Promise((resolve) => setTimeout(resolve, 0));

    // `setActiveProject` resolves against an empty `environments` list
    // here, so it lands on the "none selected" sentinel.
    bag.openProject('p1');
    assert.equal(bag.activeEnvironmentId, '');

    releaseTree();
    await booting;
    await bag.loadWorkspace();

    assert.equal(bag.activeEnvironmentId, 'env1', 'the tree layer back-fills the active env');
  });
});
