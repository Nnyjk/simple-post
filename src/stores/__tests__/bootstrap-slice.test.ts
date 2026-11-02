/**
 * Bootstrap 契约测试：锁定**两层**加载的时序，以及失败时的错误状态。
 *
 * 分层是这一版的核心，所以测试盯的不是"发了哪几个命令"，而是"欢迎页
 * 在哪一刻可以画"：`loaded` 必须在只有 `projects_list` 返回时就为 true，
 * 而 `nodesLoaded` 要等到树那一组读完。后者若和前者一起翻 true，启动画面
 * 就又回到了"等最慢的那个查询"。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { clearIpcMock, installBrowserShim, installIpcMock } from './_browser-shim';
import { createBootstrapSlice, type BootstrapSlice } from '../bootstrap-slice';
import { saveLastProjectId, saveRememberedEnvironmentId } from '../slices/ui-prefs';
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
    // 进入 / 离开项目现在把标签条交给 `ui-tabs`，而不是从这里开标签。打桩理由与
    // `openTab` 相同：那次交换本身是 `ui-tabs` 的契约，由
    // `project-tabs.test.ts` 覆盖；这些测试关心的是 `bootstrap-slice`
    // **什么时候**开口要它。
    switchProjectTabs: () => undefined,
    closeTab: () => undefined,
    setLastResponse: () => undefined,
    loadWorkspace: async () => undefined,
    // `setActiveProject` 在真正发生变化时会触发按项目的响应历史读取。打桩理由同样
    // 与 `openTab` 相同：加载本身是 `ui-response` 的契约（由
    // `ui-response-history.test.ts` 覆盖）；这些测试关心的是
    // `bootstrap-slice` **什么时候**开口要它。
    loadResponseHistory: async () => undefined,
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
  moduleBaseUrls: {},
  variables: {},
  disabledVariables: [],
  createdAt: 1,
};
/** 比 `environment` 晚创建的环境 —— 故它不是这个项目的默认环境。 */
const laterEnvironment: Environment = {
  id: 'env2',
  projectId: 'p1',
  name: '预发',
  moduleBaseUrls: {},
  variables: {},
  disabledVariables: [],
  createdAt: 2,
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
    // localStorage 里存着「上次停留的项目」。不清它，一个测试记下的项目会漏进下一个
    // 测试的启动流程，于是「启动不预选项目」那条断言会被前一个测试的残留改写。
    window.localStorage.clear();
    originalError = console.error;
    console.error = () => undefined;
  });

  afterEach(() => {
    console.error = originalError;
    clearIpcMock();
  });

  it('第一层只读 projects 与两条全局偏好：欢迎页不必等树那一组', async () => {
    const bag = makeBootstrapHarness();
    const calls: CapturedCall[] = [];
    const responses: Record<string, unknown> = {
      projects_list: [project],
      request_settings_get: { timeoutMs: 30_000, followRedirects: true, maxResponseSizeKb: 5_120 },
      response_history_keep_get: 10,
      // 树是一张扁平的表：module / collection / endpoint 三种行都由 `nodes_list` 一次返回。
      nodes_list: [moduleNode, collectionNode, endpointNode],
      endpoint_details_list: [endpointDetail],
      environments_list: [environment],
    };
    // 给树那一层设闸，好让我们能在 bootstrap 中途观察 store。树那三条还在 pending 时
    // `loaded` 就必须已经是 true。
    let releaseTree: () => void = () => undefined;
    const treeGate = new Promise<void>((resolve) => {
      releaseTree = resolve;
    });
    // 只有树那一组等闸。第 1 层读的那三条（项目清单与两条全局偏好）必须立刻返回，
    // 否则欢迎页会跟着树一起等。
    const TREE_COMMANDS = new Set(['nodes_list', 'endpoint_details_list', 'environments_list']);
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args });
      if (TREE_COMMANDS.has(cmd)) await treeGate;
      return responses[cmd];
    });

    const booting = bag.bootstrap();
    // 不释放树闸就等第一层落定。一个宏任务 tick 会排空所有 pending 微任务，
    // 而那正好是第一层 `set()` 执行的边界——数 `Promise.resolve()` 只会是对
    // `invokeCmd` 恰好含有几个 await 的猜测。
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(bag.loaded, true, 'welcome screen can paint on the project list alone');
    assert.equal(bag.nodesLoaded, false, 'tree has not landed yet');
    assert.deepEqual(bag.projects, [project]);
    // 第二层是**立刻开始**的——只是不 await 才让它离开关键路径。所以断言不是
    // "这些命令没被发出"，而是"它们还没 resolve"——`loaded` 为 true 而同时
    // `nodesLoaded` 为 false 已经证明了这一点。
    assert.deepEqual(
      calls.map((c) => c.cmd),
      [
        'projects_list',
        'request_settings_get',
        'response_history_keep_get',
        'nodes_list',
        'endpoint_details_list',
        'environments_list',
      ],
      'layer 2 is dispatched, just never awaited by bootstrap',
    );

    releaseTree();
    await booting;
    // `bootstrap()` 第一层一完成就返回——这正是这次拆分的全部意义。去 await
    // `loadWorkspace()` 反而会把这一趟并进 bootstrap 已经启动的那一趟，
    // 而后者之所以做得到，只是在于在途的加载交回它的 pending promise 而不是
    // 立即 resolve。
    await bag.loadWorkspace();

    assert.equal(bag.nodesLoaded, true);
    assert.equal(bag.nodesLoading, false);
    assert.equal(bag.bootstrapError, null);
    assert.equal(bag.nodesError, null);
    assert.deepEqual(calls.slice(3), [
      { cmd: 'nodes_list', args: {} },
      { cmd: 'endpoint_details_list', args: {} },
      { cmd: 'environments_list', args: {} },
    ]);
    assert.deepEqual(bag.nodes, [moduleNode, collectionNode, endpointNode]);
    // detail 列表按节点 id 作键，不是以数组形式带回来。
    assert.deepEqual(bag.endpointDetails, { e1: endpointDetail });
    assert.deepEqual(bag.environments, [environment]);
    // 启动时不选中任何东西——见下一个测试。
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

    // `bootstrap`、`openProject` 与重试按钮都会各自来找它，彼此不协调——在途守卫
    // 正是防止它们叠起三份同样的查询。
    const first = bag.loadWorkspace();
    const second = bag.loadWorkspace();
    assert.equal(bag.nodesLoading, true);

    releaseTree();
    await Promise.all([first, second]);

    assert.equal(calls.filter((c) => c === 'nodes_list').length, 1);
  });

  it('reloadWorkspace 绕过幂等早退：树已加载后仍重新拉取', async () => {
    const bag = makeBootstrapHarness();
    const calls: string[] = [];
    // 第一次读回旧的树。
    installIpcMock(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'nodes_list') return [moduleNode];
      return [];
    });
    await bag.loadWorkspace();
    assert.equal(bag.nodesLoaded, true);
    assert.equal((bag.nodes as ResourceNode[])[0].name, '模块');

    // 导入之后树里多了一批行。`loadWorkspace` 在 `nodesLoaded` 上早退，
    // 拿它刷新会让刚导入的东西在 store 里永远不出现。
    installIpcMock(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'nodes_list')
        return [moduleNode, { ...moduleNode, id: 'c1', kind: 'collection' }];
      return [];
    });
    await bag.reloadWorkspace();

    assert.deepEqual(
      calls.filter((c) => c === 'nodes_list'),
      ['nodes_list', 'nodes_list'],
      'reloadWorkspace 必须真发第二轮 nodes_list',
    );
    assert.equal((bag.nodes as ResourceNode[]).length, 2);
  });

  it('reloadWorkspace 之后 loadWorkspace 重新变回幂等', async () => {
    const bag = makeBootstrapHarness();
    let trees: ResourceNode[] = [moduleNode];
    const calls: string[] = [];
    installIpcMock(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'nodes_list') return trees;
      return [];
    });

    await bag.reloadWorkspace();
    trees = [moduleNode, { ...moduleNode, id: 'c1', kind: 'collection' }];
    await bag.reloadWorkspace();
    // 重载已把 `nodesLoaded` 置真，故随后的 `loadWorkspace` 仍应早退。
    await bag.loadWorkspace();

    assert.equal(calls.filter((c) => c === 'nodes_list').length, 2);
    assert.equal((bag.nodes as ResourceNode[]).length, 2);
  });

  it('reloadWorkspace 失败落 nodesError，不 reject 也不清空已有的树', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => (cmd === 'nodes_list' ? [moduleNode] : []));
    await bag.loadWorkspace();

    installIpcMock(async (cmd) => {
      if (cmd === 'nodes_list') throw new Error('读树失败');
      return [];
    });
    await bag.reloadWorkspace();

    assert.equal(bag.nodesError, '读树失败');
    // 已有的树仍可读 —— 刷新失败不该让整个工作区变成空白。
    assert.equal((bag.nodes as ResourceNode[]).length, 1);
    assert.equal(bag.nodesLoaded, true);
    assert.equal(bag.nodesLoading, false);
  });

  it('两趟 reloadWorkspace 重叠时，先结束的那趟不清掉在途槽位', async () => {
    // 槽位（`inFlight`）只有一个，先结束的那趟若无条件清空，后面那一趟明明还在
    // 跑，下一个 `loadWorkspace` 却会以为没人占用槽位，再发一轮同样的 IPC。
    const bag = makeBootstrapHarness();
    let nodesCalls = 0;
    let releaseSecond: () => void = () => undefined;
    const secondGate = new Promise<void>((resolve) => {
      releaseSecond = resolve;
    });
    installIpcMock(async (cmd) => {
      if (cmd !== 'nodes_list') return [];
      nodesCalls += 1;
      // 第一趟失败（`nodesLoaded` 保持 false），第二趟慢。
      if (nodesCalls === 1) throw new Error('读树失败');
      await secondGate;
      return [];
    });

    const first = bag.reloadWorkspace();
    const second = bag.reloadWorkspace();
    await first;

    assert.equal(bag.nodesError, null, '被后一趟取代的失败是过期消息，不该挂在屏幕上');
    assert.equal(bag.nodesLoaded, false, '失败的那一趟不置位 nodesLoaded');

    // 此刻第二趟还在途。槽位若已被第一趟清掉，这一句会再开一趟，IPC 就多一轮。
    const third = bag.loadWorkspace();
    let thirdSettled = false;
    void third.then(() => {
      thirdSettled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(nodesCalls, 2, '在途槽位仍被第二趟占着，不该发出第三轮 IPC');
    assert.equal(thirdSettled, false, 'loadWorkspace 应当接着第二趟，而不是新开一趟');

    releaseSecond();
    await Promise.all([second, third]);

    assert.equal(bag.nodesLoaded, true);
    // 树读回来了，那条过期的失败消息不该还挂着。
    assert.equal(bag.nodesError, null, '成功之后不留过期错误');
    assert.equal(nodesCalls, 2);
  });

  it('reloadWorkspace 保留展开状态：整份替换节点不该把用户打开的分支收起', async () => {
    // 展开状态只活在渲染器里，服务端不存它。故重取一趟树若直接替换，用户此前
    // 打开的分支就全塌了——导入一次就足以看见。
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => (cmd === 'nodes_list' ? [moduleNode] : []));
    await bag.loadWorkspace();

    // 用户展开了这个模块。
    bag.toggleNodeExpanded('m1');
    assert.equal((bag.nodes as ResourceNode[])[0].expanded, true);

    // 导入之后重取一趟树：服务端那份不带 `expanded`（它不落库），新集合也没有。
    installIpcMock(async (cmd) =>
      cmd === 'nodes_list' ? [moduleNode, { ...moduleNode, id: 'c1', kind: 'collection' }] : [],
    );
    await bag.reloadWorkspace();

    const nodes = bag.nodes as ResourceNode[];
    assert.equal(nodes[0].expanded, true, '展开状态按 id 合回去了');
    assert.equal(nodes[1].expanded, undefined, '新出现的节点保持收起');
  });

  it('refreshProjects 整份替换项目清单：导入后新建的项目要立刻看得见', async () => {
    // 项目行是 bootstrap 第一层加载后就没有人再更新的那份副本。导入在服务端建项目后
    // 不刷新它，新项目在界面上就是不存在。
    const bag = makeBootstrapHarness();
    const imported: Project = {
      ...project,
      id: 'p2',
      name: '导入的项目',
    };
    installIpcMock(async (cmd) => (cmd === 'projects_list' ? [imported] : []));

    await bag.refreshProjects();

    assert.deepEqual(bag.projects, [imported]);
  });

  it('refreshProjects 失败不 reject：导入已经落库，不能因为一次刷新失败被报成失败', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') throw new Error('读项目失败');
      return [];
    });

    await bag.refreshProjects();

    // 失败只记日志：项目清单保持原样，导入的结果仍然是真的。
    assert.deepEqual(bag.projects, []);
  });

  it('启动不预选任何项目或接口，把选择权留给欢迎页', async () => {
    const bag = makeBootstrapHarness();
    const otherProject: Project = { ...project, id: 'p2', name: '别的项目' };
    // 两个项目都有内容。自动选中某个项目（连同它的第一个 endpoint）会让有多项目的
    // 用户既看不到选择页，也看不到自己被丢进去的那个项目。
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

  it('刷新后回到上次停留的项目', async () => {
    // 刷新丢的是「我在哪个项目里干活」，不是项目本身。故它记在 localStorage 里，
    // 启动时据此回到那个项目，而不是回到欢迎页。
    const bag = makeBootstrapHarness();
    window.localStorage.setItem('simple-post:prefs', JSON.stringify({ lastProjectId: 'p1' }));
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.activeProjectId, 'p1', '刷新后回到上次停留的项目');
  });

  it('没有记住的项目时仍然留在欢迎页', async () => {
    const bag = makeBootstrapHarness();
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.activeProjectId, '', '没记住项目就不进项目');
  });

  it('记住的项目已不存在时清掉那条记录并留在欢迎页', async () => {
    const bag = makeBootstrapHarness();
    window.localStorage.setItem('simple-post:prefs', JSON.stringify({ lastProjectId: 'gone' }));
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      return [];
    });

    await bag.bootstrap();

    assert.equal(bag.activeProjectId, '', '进不去的项目不该被恢复');
    const prefs = JSON.parse(window.localStorage.getItem('simple-post:prefs') ?? '{}');
    assert.equal(prefs.lastProjectId, undefined, '记着的那条要被清掉，否则每次刷新都白试一次');
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
    // 用户在后台那一趟完成之前就点了项目卡片，也必须仍能拿到树，
    // 所以进入项目会重新请求。
    bag.loadWorkspace = async () => {
      workspaceLoads += 1;
    };
    installIpcMock(async (cmd) => (cmd === 'projects_list' ? [project] : []));

    bag.openProject('p1');

    // 切换发生在选中**之前**，这个次序就是重点：把出去那个项目的标签暂存起来，
    // 需要先回答每个标签属于哪个项目，而这个问题只有当出去的那个项目还在前面时
    // 才答得出来。
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

    // `App.tsx` 在 `activeProjectId === ''` 时切换表面，所以清空它就是
    // "回到欢迎页"的全部。
    assert.equal(bag.activeProjectId, '');
    assert.equal(
      bag.activeEnvironmentId,
      '',
      'an env from the abandoned project is meaningless here',
    );
    assert.equal(bag.activeEndpointId, null);
    // 重新进入不得重拉：树是按项目的状态，用户下次点击时多半还想要它。
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
      // 树仍然渲染；只有 HTTP 测试器是空的。
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

    // 这正是这次拆分的分界点：第一层成功就足以撑起欢迎页，所以即便树那一层失败，
    // `loaded` 仍为 true。把两个 flag 重新合在一起，要么会把启动画面卡住，
    // 要么会掩盖一次真实的树失败。
    assert.equal(bag.loaded, true, 'welcome screen still paints');
    assert.deepEqual(bag.projects, [project]);
    assert.equal(bag.nodesLoaded, false);
    assert.equal(bag.nodesLoading, false);
    assert.equal(bag.bootstrapError, null, 'not reported as a startup failure');
    assert.equal(bag.nodesError, 'environment store offline');
    assert.deepEqual(bag.environments, []);
  });

  it('第二层在项目已进入后才返回时，补选该项目的 active 环境', async () => {
    const bag = makeBootstrapHarness();
    let releaseTree: () => void = () => undefined;
    const treeGate = new Promise<void>((resolve) => {
      releaseTree = resolve;
    });
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      if (cmd === 'environments_list') {
        // 在用户已经点了卡片之后才到达。
        await treeGate;
        return [environment];
      }
      return [];
    });

    const booting = bag.bootstrap();
    await new Promise((resolve) => setTimeout(resolve, 0));

    // 这里 `setActiveProject` 是对着空的 `environments` 列表解析的，
    // 所以落在"未选中"哨兵上。
    bag.openProject('p1');
    assert.equal(bag.activeEnvironmentId, '');

    releaseTree();
    await booting;
    await bag.loadWorkspace();

    assert.equal(bag.activeEnvironmentId, 'env1', 'the tree layer back-fills the current env');
  });

  it('第二层补齐时用记住过的那个环境，而不是默认环境', async () => {
    const bag = makeBootstrapHarness();
    // 本客户端上次在这个项目里用的是 env2（比默认的 env1 晚创建）。
    saveLastProjectId('p1');
    saveRememberedEnvironmentId('p1', 'env2');
    installIpcMock(async (cmd) => {
      if (cmd === 'projects_list') return [project];
      if (cmd === 'environments_list') return [environment, laterEnvironment];
      return [];
    });

    await bag.bootstrap();
    await bag.loadWorkspace();

    assert.equal(
      bag.activeEnvironmentId,
      'env2',
      '记住过的那个环境优先于默认环境——刷新后该停在同一个环境',
    );
  });
});
