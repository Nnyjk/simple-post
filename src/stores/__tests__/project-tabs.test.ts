/**
 * Tests for the per-project tab strip — `switchProjectTabs` in
 * `src/stores/slices/ui-tabs.ts`, driven through `openProject` /
 * `leaveProject` so the ordering those two rely on is exercised too.
 *
 * WHY THE REAL COMPOSED STORE
 * ---------------------------
 * The behaviour under test is a contract *between* slices: `ui-tabs`
 * decides which tabs a project owns (it has to read `nodes` and
 * `draftEndpoints` to answer that), while `bootstrap-slice` owns
 * `activeProjectId` and must call the switch before moving it. A harness
 * that stubbed either side would stub away the thing most likely to
 * break.
 *
 * The load is stubbed out entirely (`nodesLoaded: true`) because it is
 * irrelevant here: `nodes` holds every project's rows at once and the
 * tree filters them by `rootId`, which is exactly why switching projects
 * can restore a tab strip with no round-trip at all.
 *
 * Run via: npx tsx --test src/stores/__tests__/project-tabs.test.ts
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

  // Nothing should reach the daemon: the tree is already loaded and
  // switching projects never refetches it. A call here would mean a
  // switch had started depending on a round-trip.
  installIpcMock((cmd) => {
    throw new Error(`unexpected command during a project switch: ${cmd}`);
  });

  useAppStore.setState({
    projects: [],
    nodes: [M1, E1, M2, E2],
    endpointDetails: {},
    // The whole point: everything is already in memory, per project.
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

/** Enter project A, open an endpoint tab in it, and dirty it. */
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
