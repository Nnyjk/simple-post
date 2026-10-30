/**
 * 「记住上次停留的项目」与 `activeProjectId` 必须始终一致。
 *
 * 记住的那条是渲染层的会话事实（localStorage 里的 `lastProjectId`），刷新后据此
 * 回到那个项目。它由 store 的订阅维护（见 `app-store.ts`），故这里测的是**每一条**
 * 改 `activeProjectId` 的路径，而不是某一个动作。
 *
 * **为什么用真实组合 store。** 被测的正是订阅：它挂在真实 store 上，挂在各个
 * 切片动作里则会在假 store 的测试里整个失效 —— 那种测试会把最容易坏的一半
 * stub 掉。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { readLastProjectId } from '../slices/ui-prefs';
import type { Project } from '@/types/domain';

function project(id: string, name: string): Project {
  return {
    id,
    name,
    color: '#000000',
    createdAt: 0,
    updatedAt: 0,
  };
}

const P1 = project('p1', '项目一');
const P2 = project('p2', '项目二');

let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  // 记住的项目存在 localStorage 里；不清它，一个测试记下的项目会漏进下一个测试。
  window.localStorage.clear();
  origError = console.error.bind(console);
  console.error = () => {};

  useAppStore.setState({
    projects: [P1, P2],
    nodes: [],
    endpointDetails: {},
    environments: [],
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

describe('记住的项目与 activeProjectId 始终一致', () => {
  it('进入项目就记住它', () => {
    useAppStore.getState().openProject('p1');
    assert.equal(readLastProjectId(), 'p1');
  });

  it('命令面板跨项目跳接口走的那条路也记住它', () => {
    // 命令面板的「跳到接口」用 `setActiveProject` 而不是 `openProject`（换条会把它
    // 紧接着要开的接口标签顶掉）。于是 `activeProjectId` 变了而标签条没换 ——
    // 记住的那条必须跟着变，否则刷新回到的是另一个项目。
    useAppStore.getState().openProject('p1');
    useAppStore.getState().setActiveProject('p2');

    assert.equal(useAppStore.getState().activeProjectId, 'p2');
    assert.equal(readLastProjectId(), 'p2', '记住的项目要跟着实际所在的项目走');
  });

  it('删掉在前面的项目后，记住的换成幸存的那个', async () => {
    installIpcMock(async () => null);
    useAppStore.getState().openProject('p1');
    assert.equal(readLastProjectId(), 'p1');

    await useAppStore.getState().deleteProject('p1');

    assert.equal(useAppStore.getState().activeProjectId, 'p2', '用户被送到幸存的项目');
    assert.equal(readLastProjectId(), 'p2', '记住的也要换，否则刷新回到欢迎页');
  });

  it('删除失败回滚时，记住的值跟着回滚', async () => {
    installIpcMock(async () => {
      throw new Error('删除失败');
    });
    useAppStore.getState().openProject('p1');

    await useAppStore.getState().deleteProject('p1');

    assert.equal(useAppStore.getState().activeProjectId, 'p1', '回滚把项目还回来了');
    assert.equal(readLastProjectId(), 'p1', '记住的也要跟着回滚');
  });

  it('退出项目就不再记着它', () => {
    useAppStore.getState().openProject('p1');
    useAppStore.getState().leaveProject();

    assert.equal(useAppStore.getState().activeProjectId, '');
    assert.equal(readLastProjectId(), '', '不在任何项目里，刷新后回欢迎页');
  });
});
