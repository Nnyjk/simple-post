/**
 * 按项目划分的标签条测试——`src/stores/slices/ui-tabs.ts` 的 `switchProjectTabs`，
 * 通过 `openProject` / `leaveProject` 驱动，好让这两者依赖的调用次序也被跑到。
 *
 * **为什么用真实组合 store。** 被测行为是切片**之间**的契约：`ui-tabs` 决定
 * 哪些标签属于某个项目（为此必须读 `nodes` 和 `draftEndpoints`），
 * `bootstrap-slice` 则拥有 `activeProjectId` 且必须在移动它之前调用切换。
 * stub 掉任何一侧的 harness 都会把最容易坏的东西 stub 掉。
 *
 * 加载被整个 stub 掉（`nodesLoaded: true`），因为这里与它无关：`nodes` 一次
 * 持有所有项目的行，树按 `rootId` 过滤它们——这也正是切项目可以零往返地恢复
 * 一条标签条的原因。
 *
 * 运行：npx tsx --test src/stores/__tests__/project-tabs.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { SETTINGS_TAB_ID } from '../store-types';
import type { ResourceNode } from '@/types/domain';

function node(over: Partial<ResourceNode> & Pick<ResourceNode, 'id' | 'rootId'>): ResourceNode {
  return {
    parentId: null,
    kind: 'module',
    name: over.id,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    ...over,
  };
}

const M1 = node({ id: 'm1', rootId: 'p1', name: '支付' });
const E1 = node({ id: 'e1', rootId: 'p1', parentId: 'm1', kind: 'endpoint', name: '下单' });
const M2 = node({ id: 'm2', rootId: 'p2', name: '库存' });
const E2 = node({ id: 'e2', rootId: 'p2', parentId: 'm2', kind: 'endpoint', name: '扣减' });

let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  origError = console.error.bind(console);
  console.error = () => {};

  // 不该有任何请求打到守护进程：树已经加载过，切项目从不重拉它。这里若出现调用，
  // 就说明切换开始依赖某次往返了。
  installIpcMock((cmd) => {
    throw new Error(`unexpected command during a project switch: ${cmd}`);
  });

  useAppStore.setState({
    projects: [],
    nodes: [M1, E1, M2, E2],
    endpointDetails: {},
    // 重点就在这里：一切都已按项目留在内存里了。
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

const tabIds = () => useAppStore.getState().openTabs.map((t) => t.id);
const tabFor = (id: string) => useAppStore.getState().openTabs.find((t) => t.id === id);

/** 进入项目 A，在里面开一个 endpoint 标签并把它标脏。 */
function enterAWithADirtyEndpoint() {
  const store = useAppStore.getState();
  store.openProject('p1');
  store.setActiveEndpoint('e1');
  store.setEndpointDraft('e1', { url: '/未保存的路径' });
}

describe('switching projects: the strip shows only the project in front', () => {
  it('sets the outgoing project’s tabs aside', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().openProject('p2');

    const state = useAppStore.getState();
    assert.equal(tabFor('e1'), undefined, "project A's endpoint tab must not survive the switch");
    assert.ok(!tabIds().includes('e1'), 'not merely deactivated — gone from the strip');
    assert.deepEqual(
      state.projectTabs.p1?.tabs.map((t) => t.id).sort(),
      ['e1', 'p1'],
      'and kept aside for when the user comes back',
    );
  });

  it('gives the incoming project its own project tab', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().openProject('p2');

    const state = useAppStore.getState();
    assert.deepEqual(tabIds().sort(), ['p2'], 'a fresh project starts with just its own tab');
    assert.equal(state.activeTabId, 'p2');
  });

  it('restores the tabs, the unsaved edits and the dirty flag on the way back', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().openProject('p2');
    useAppStore.getState().openProject('p1');

    const state = useAppStore.getState();
    assert.ok(tabFor('e1'), 'the endpoint tab is back');
    assert.equal(
      state.endpointDrafts.e1?.url,
      '/未保存的路径',
      'with the edit the user never saved',
    );
    assert.equal(state.dirtyTabIds.has('e1'), true, 'and still marked dirty');
  });

  it('puts the user back where they left, not on the project card', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().openProject('p2');
    useAppStore.getState().openProject('p1');

    assert.equal(useAppStore.getState().activeTabId, 'e1');
  });

  it('clears the active endpoint, which belonged to the project being left', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().openProject('p2');

    assert.equal(
      useAppStore.getState().activeEndpointId,
      null,
      'otherwise the tester would resolve an endpoint the new tree does not contain',
    );
  });

  it('is a no-op when re-entering the project already in front', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().openProject('p1');

    const state = useAppStore.getState();
    assert.deepEqual(tabIds().sort(), ['e1', 'p1'], 'nothing is stashed or re-restored');
    assert.equal(state.activeTabId, 'e1', 'and the active tab is untouched');
    assert.deepEqual(state.projectTabs, {}, 'no stray cache entry');
  });
});

describe('the settings tab belongs to the user, not to a project', () => {
  it('survives a switch instead of being stashed', () => {
    enterAWithADirtyEndpoint();
    useAppStore.getState().openSettingsTab();

    useAppStore.getState().openProject('p2');

    const state = useAppStore.getState();
    assert.ok(tabFor(SETTINGS_TAB_ID), 'settings follows the user across projects');
    assert.equal(
      state.projectTabs.p1?.tabs.some((t) => t.id === SETTINGS_TAB_ID),
      false,
    );
  });
});

describe('a draft endpoint is owned by the project it was created under', () => {
  it('is stashed with that project even though it has no row in `nodes`', () => {
    const store = useAppStore.getState();
    store.openProject('p1');
    const draftId = store.createEndpointDraft('m1');
    store.setEndpointDraft(draftId, { body: { mode: 'json', content: '{"a":1}' } });

    useAppStore.getState().openProject('p2');
    assert.equal(tabFor(draftId), undefined, 'not on the other project’s strip');

    useAppStore.getState().openProject('p1');
    const state = useAppStore.getState();
    assert.equal(tabFor(draftId)?.kind, 'endpoint-draft', 'and back where it was created');
    assert.equal(
      state.draftEndpoints[draftId]?.rootId,
      'p1',
      'ownership comes from the rootId snapshotted on the draft',
    );
    assert.equal(state.endpointDrafts[draftId]?.body?.content, '{"a":1}', 'unsaved body kept');
  });
});

describe('leaving for the welcome screen', () => {
  it('sets the strip aside and empties it', () => {
    enterAWithADirtyEndpoint();

    useAppStore.getState().leaveProject();

    const state = useAppStore.getState();
    assert.deepEqual(tabIds(), [], 'nothing of the project remains on screen');
    assert.equal(state.activeTabId, null);
    assert.deepEqual(
      state.projectTabs.p1?.tabs.map((t) => t.id).sort(),
      ['e1', 'p1'],
      'and it is all still there to come back to',
    );
    assert.equal(state.endpointDrafts.e1?.url, '/未保存的路径', 'unsaved work survives leaving');
  });

  it('does nothing when no project is open', () => {
    useAppStore.getState().leaveProject();

    assert.equal(useAppStore.getState().activeTabId, null);
  });
});
