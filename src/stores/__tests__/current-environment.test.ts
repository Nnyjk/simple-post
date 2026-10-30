/**
 * 「当前环境」是本客户端的实时状态，记住的那条与 `activeEnvironmentId` 必须始终一致。
 *
 * 记住的那条是渲染层的会话事实（localStorage 里的 `currentEnvByProject`），刷新后据此
 * 停在这个项目里用过的那个环境。它由 store 的订阅维护（见 `app-store.ts`），故这里测
 * 的是**每一条**改 `activeEnvironmentId` 的路径，而不是某一个动作。
 *
 * **为什么用真实组合 store。** 被测的正是订阅：它挂在真实 store 上，挂在各个切片
 * 动作里则会在假 store 的测试里整个失效 —— 那种测试会把最容易坏的一半 stub 掉。
 *
 * **为什么还要钉「不发 IPC」。** 当前环境不落库：落库会让 web 端多个人互相踩 ——
 * 一个人切了环境，另一个人发请求就换了地址与端口。命令面板与顶栏都只改本地状态。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { readRememberedEnvironmentId } from '../slices/ui-prefs';
import type { Environment, Project } from '@/types/domain';

function project(id: string, name: string): Project {
  return {
    id,
    name,
    color: '#000000',
    createdAt: 0,
    updatedAt: 0,
  };
}

/** 一个环境。`createdAt` 决定它是不是这个项目的默认环境（最早创建的那个）。 */
function env(id: string, projectId: string, name: string, createdAt: number): Environment {
  return {
    id,
    projectId,
    name,
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    createdAt,
  };
}

const P1 = project('p1', '项目一');
const P2 = project('p2', '项目二');
/** p1 的两个环境：dev 先创建（故是 p1 的默认环境），staging 后创建。 */
const E1_DEV = env('e1', 'p1', 'dev', 100);
const E1_STAGING = env('e2', 'p1', 'staging', 200);
const E2_DEV = env('e3', 'p2', 'dev', 100);
/** p2 的第二个环境：dev 先创建（故是 p2 的默认环境），staging 后创建。 */
const E2_STAGING = env('e4', 'p2', 'staging', 200);

let origError: typeof console.error;
/** 本次测试里发出去过的 IPC 命令名。 */
let ipcCommands: string[] = [];

beforeEach(() => {
  installBrowserShim();
  // 记住的环境存在 localStorage 里；不清它，一个测试记下的环境会漏进下一个测试。
  window.localStorage.clear();
  origError = console.error.bind(console);
  console.error = () => {};
  ipcCommands = [];

  useAppStore.setState({
    projects: [P1, P2],
    nodes: [],
    endpointDetails: {},
    environments: [E1_DEV, E1_STAGING, E2_DEV, E2_STAGING],
    nodesLoaded: true,
    activeProjectId: '',
    activeEnvironmentId: '',
    activeEndpointId: null,
    openTabs: [],
    activeTabId: null,
    projectTabs: {},
    dirtyTabIds: new Set<string>(),
    endpointDrafts: {},
    draftEndpoints: {},
    draftSaveError: null,
    lastCreatedId: null,
    lastResponse: null,
  });
});

afterEach(() => {
  console.error = origError;
  clearIpcMock();
});

describe('当前环境：记住的那条与 activeEnvironmentId 始终一致', () => {
  it('切换当前环境就记住它，且不发任何 IPC', () => {
    installIpcMock(async (cmd) => {
      ipcCommands.push(cmd);
      return null;
    });
    useAppStore.getState().setActiveProject('p1');

    useAppStore.getState().setCurrentEnvironment('e2');

    assert.equal(useAppStore.getState().activeEnvironmentId, 'e2');
    assert.equal(readRememberedEnvironmentId('p1'), 'e2', '记住的那条要跟着当前环境走');
    assert.deepEqual(
      ipcCommands.filter((c) => c.startsWith('environments_')),
      [],
      '当前环境不落库，故不该有写环境的 IPC',
    );
  });

  it('进入项目时用记住过的那个环境', () => {
    useAppStore.getState().setActiveProject('p1');
    useAppStore.getState().setCurrentEnvironment('e2');

    // 离开再回来：记住的那条还在。
    useAppStore.getState().leaveProject();
    useAppStore.getState().setActiveProject('p1');

    assert.equal(useAppStore.getState().activeEnvironmentId, 'e2', '回到项目里仍停在同一个环境');
  });

  it('没记住过就用这个项目的默认环境（最早创建的那个）', () => {
    useAppStore.getState().setActiveProject('p1');

    assert.equal(
      useAppStore.getState().activeEnvironmentId,
      'e1',
      '默认环境是最早创建的那个（dev 先于 staging）',
    );
  });

  it('记住的那个已被删掉时，改用默认环境', () => {
    useAppStore.getState().setActiveProject('p1');
    useAppStore.getState().setCurrentEnvironment('e2');
    assert.equal(readRememberedEnvironmentId('p1'), 'e2');

    // 记住的那个环境不在了（被删或被换掉了），落到默认环境。
    useAppStore.setState({
      environments: [E1_DEV, E2_DEV],
      activeEnvironmentId: 'e2',
    });
    useAppStore.getState().setActiveProject('p1');

    assert.equal(useAppStore.getState().activeEnvironmentId, 'e1', '记住的环境没了就用默认环境');
  });

  it('启动那个窗口里环境还没读到，不该把记住的环境抹掉', () => {
    useAppStore.getState().setActiveProject('p1');
    useAppStore.getState().setCurrentEnvironment('e2');
    assert.equal(readRememberedEnvironmentId('p1'), 'e2');

    // 模拟启动：环境列表还没读到，切进项目只能把当前环境清成空串（"还没定"哨兵）。
    useAppStore.setState({ environments: [], activeProjectId: '', activeEnvironmentId: '' });
    useAppStore.getState().setActiveProject('p1');

    assert.equal(
      readRememberedEnvironmentId('p1'),
      'e2',
      '那个空窗口不该把记住的环境抹掉——抹掉后刷新就回默认环境，而不是上次用的那个',
    );
  });

  it('退出项目不清掉记住的环境', () => {
    useAppStore.getState().setActiveProject('p1');
    useAppStore.getState().setCurrentEnvironment('e2');

    useAppStore.getState().leaveProject();

    assert.equal(useAppStore.getState().activeEnvironmentId, '');
    assert.equal(
      readRememberedEnvironmentId('p1'),
      'e2',
      '记住的是「在这个项目里用哪个环境」，退出项目不该抹掉它',
    );
  });

  it('每个项目各记各的', () => {
    useAppStore.getState().setActiveProject('p1');
    useAppStore.getState().setCurrentEnvironment('e2');

    useAppStore.getState().setActiveProject('p2');

    assert.equal(useAppStore.getState().activeEnvironmentId, 'e3', 'p2 用它自己的默认环境');
    assert.equal(readRememberedEnvironmentId('p1'), 'e2', 'p1 记住的那个不受影响');
    assert.equal(readRememberedEnvironmentId('p2'), 'e3');
  });

  it('删掉当前环境时回落到默认环境，并记住回落后的那个', async () => {
    installIpcMock(async () => null);
    useAppStore.getState().setActiveProject('p1');
    useAppStore.getState().setCurrentEnvironment('e2');

    await useAppStore.getState().deleteEnvironment('e2');

    assert.equal(useAppStore.getState().activeEnvironmentId, 'e1', '当前环境被删掉后落到默认环境');
    assert.equal(readRememberedEnvironmentId('p1'), 'e1', '记住的也要跟着回落');
  });

  it('切到别的项目的环境一律忽略', () => {
    useAppStore.getState().setActiveProject('p1');

    useAppStore.getState().setCurrentEnvironment('e3');

    assert.equal(
      useAppStore.getState().activeEnvironmentId,
      'e1',
      '别的项目的环境会让接口读到不属于本项目模块的格子',
    );
  });

  it('删掉当前项目时，当前环境跟着换到幸存项目上', async () => {
    installIpcMock(async () => null);
    // 先在 p2 里用 staging 记住它。
    useAppStore.getState().setActiveProject('p2');
    useAppStore.getState().setCurrentEnvironment('e4');

    // 切到 p1（它在前面的项目里），再把它删掉：用户被送到幸存的 p2。
    useAppStore.getState().setActiveProject('p1');
    assert.equal(useAppStore.getState().activeEnvironmentId, 'e1');

    await useAppStore.getState().deleteProject('p1');

    assert.equal(useAppStore.getState().activeProjectId, 'p2');
    assert.equal(
      useAppStore.getState().activeEnvironmentId,
      'e4',
      '当前环境原先指着已删项目的环境；不换的话幸存的 p2 没有环境可读',
    );
  });

  it('删掉当前项目时，幸存项目记住的环境不被顶掉', async () => {
    installIpcMock(async () => null);
    useAppStore.getState().setActiveProject('p2');
    useAppStore.getState().setCurrentEnvironment('e4');
    assert.equal(readRememberedEnvironmentId('p2'), 'e4');

    useAppStore.getState().setActiveProject('p1');
    await useAppStore.getState().deleteProject('p1');

    assert.equal(
      readRememberedEnvironmentId('p2'),
      'e4',
      'p2 自己记住的环境不能被已删项目的环境 id 顶掉',
    );
  });
});
