/**
 * Slice-level test for `src/stores/slices/ui-new-item.ts` — the create
 * paths' parent choice.
 *
 * Two entry points are covered:
 *
 *   - `createEndpointAt(parentId)` — no dialog, explicit parent. This is
 *     where endpoints are born now, as a frontend draft: nothing reaches
 *     SQLite until `saveDraftEndpoint`, so the assertions here are about
 *     *not* writing rather than about what was written.
 *   - `submitNewItem()` — the three dialog kinds (project / module /
 *     collection).
 *
 * The parent/child contract gives an endpoint two legal parents (a module
 * or a collection) and a collection the same two, so both paths must
 * resolve the parent from somewhere and refuse when they cannot:
 *
 *   - `createEndpointAt`: the id it was handed, validated against `nodes`
 *   - `submitNewItem`: `parentCollectionId || parentModuleId`, same
 *     validation
 *
 * The "refuse" case is the guard that must survive, because a node with
 * no parent is not a tree — and for an endpoint now, because a draft whose
 * parent never existed can never be saved.
 *
 * Run via:
 *   npx tsx --test src/stores/__tests__/ui-new-item.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createUiNewItem, UNTITLED_ENDPOINT_NAME } from '../slices/ui-new-item';
import { createUiDraftEndpoints } from '../slices/ui-draft-endpoints';
import type { NewItemDraft } from '../store-types';
import type { ResourceNode } from '@/types/domain';

/** What `addNode` was asked to create, captured for assertions. */
interface AddNodeCall {
  rootId?: string;
  parentId: string | null;
  kind: string;
  name: string;
  description?: string;
  detail?: { url: string };
}

function makeNode(
  id: string,
  parentId: string | null,
  kind: ResourceNode['kind'],
  expanded = true,
): ResourceNode {
  return {
    id,
    rootId: 'p1',
    parentId,
    kind,
    name: id,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    expanded,
  };
}

function draft(overrides: Partial<NewItemDraft> = {}): NewItemDraft {
  return {
    open: true,
    type: 'collection',
    parentModuleId: '',
    parentCollectionId: '',
    name: 'n',
    description: '',
    color: '#000000',
    ...overrides,
  };
}

/**
 * A minimal store for the two actions under test. They only reach for the
 * node list, the expand toggle, `addNode` / `addProject` /
 * `setActiveEndpoint` / `openProject`, so everything else is a stub.
 *
 * `createUiDraftEndpoints` is composed in for real rather than stubbed:
 * `createEndpointAt` delegates to `createEndpointDraft`, and a stub there
 * would test that the call happened instead of that a draft came out.
 */
function makeHarness(nodeList: ResourceNode[]) {
  const calls: {
    addNode: AddNodeCall[];
    expanded: string[];
    activeEndpoint: (string | null)[];
    activeProject: string[];
    addProject: { name: string; opts: unknown }[];
    openTab: { kind: string; refId: string }[];
  } = {
    addNode: [],
    expanded: [],
    activeEndpoint: [],
    activeProject: [],
    addProject: [],
    openTab: [],
  };
  const bag = {
    nodes: nodeList,
    newItem: draft(),
    activeProjectId: 'p1',
    lastCreatedId: null as string | null,
    draftEndpoints: {} as Record<string, unknown>,
    endpointDrafts: {} as Record<string, unknown>,
    dirtyTabIds: new Set<string>(),
    openTabs: [] as { id: string; kind: string }[],
    activeTabId: null as string | null,
    addProject: async (name: string, opts: unknown) => {
      calls.addProject.push({ name, opts });
      return 'proj-1';
    },
    addNode: async (input: AddNodeCall) => {
      calls.addNode.push(input);
      return 'node-new';
    },
    setActiveProject: () => {},
    openProject: (id: string) => calls.activeProject.push(id),
    setActiveEndpoint: (id: string | null) => calls.activeEndpoint.push(id),
    toggleNodeExpanded: (id: string) => calls.expanded.push(id),
    openTab: (kind: string, refId: string) => {
      calls.openTab.push({ kind, refId });
      bag.openTabs = [...bag.openTabs, { id: refId, kind }];
      bag.activeTabId = refId;
    },
  };
  const set = (partial: unknown) => {
    const updater =
      typeof partial === 'function' ? (partial as (state: typeof bag) => unknown)(bag) : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = {
    ...createUiNewItem(set as never, get as never, {} as never),
    ...createUiDraftEndpoints(set as never, get as never, {} as never),
  };
  // In the real store an action reaches its siblings through `get()`, so
  // the composed actions have to sit on the object `get` returns — not
  // beside it.
  Object.assign(bag, slice);
  return { bag, calls, slice };
}

describe('ui-new-item: createEndpointAt takes the parent from its argument', () => {
  it('creates an untitled draft under the module that was clicked, writing nothing', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const draftId = await h.slice.createEndpointAt('m1');

    // A placeholder that reads as provisional, not as a final name.
    assert.ok(draftId, 'a valid parent must yield a draft id');
    const draft = h.bag.draftEndpoints[draftId] as { name: string; parentId: string };
    assert.equal(draft.name, UNTITLED_ENDPOINT_NAME);
    assert.equal(draft.parentId, 'm1');
    // The contract change: no row, and no daemon call behind it. The tree
    // gains nothing until Save.
    assert.equal(h.calls.addNode.length, 0, 'a draft must not reach the database');
    assert.equal(h.bag.nodes.length, 1);
  });

  it('creates inside the collection when one was clicked', async () => {
    const h = makeHarness([makeNode('m1', null, 'module'), makeNode('c1', 'm1', 'collection')]);

    const draftId = await h.slice.createEndpointAt('c1');

    assert.ok(draftId, 'a valid parent must yield a draft id');
    assert.equal((h.bag.draftEndpoints[draftId] as { parentId: string }).parentId, 'c1');
  });

  it('refuses when the parent is unknown', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const id = await h.slice.createEndpointAt('m-gone');

    assert.equal(id, null, 'a stale id must not write an orphan the tree cannot render');
    assert.equal(h.calls.addNode.length, 0, 'nothing may be written');
    assert.deepEqual(h.bag.draftEndpoints, {});
  });

  it('refuses a parent that cannot hold an endpoint', async () => {
    // A project is not a tree node, but the check is on `kind` alone so a
    // future node type is rejected by default rather than accepted.
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const id = await h.slice.createEndpointAt('m1-nope');

    assert.equal(id, null);
    assert.equal(h.calls.addNode.length, 0);
  });

  it('refuses an empty parent id without touching the store', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const id = await h.slice.createEndpointAt('');

    assert.equal(id, null);
    assert.equal(h.calls.addNode.length, 0);
  });

  it('expands the module it landed on so the new row is visible', async () => {
    const h = makeHarness([makeNode('m1', null, 'module', false)]);

    await h.slice.createEndpointAt('m1');

    assert.deepEqual(h.calls.expanded, ['m1']);
  });

  it('walks every ancestor of a nested collection, nearest first', async () => {
    const h = makeHarness([
      makeNode('m1', null, 'module', false),
      makeNode('c1', 'm1', 'collection', false),
      makeNode('c2', 'c1', 'collection', false),
    ]);

    await h.slice.createEndpointAt('c2');

    // The collection itself, then its parent, then the module.
    assert.deepEqual(h.calls.expanded, ['c2', 'c1', 'm1']);
  });

  it('opens a draft tab — not an endpoint tab, which has no row to show', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const draftId = await h.slice.createEndpointAt('m1');

    // `setActiveEndpoint` would open an `endpoint` tab for an id nothing in
    // `nodes` answers to, so the draft opens its own kind instead.
    assert.deepEqual(h.calls.openTab, [{ kind: 'endpoint-draft', refId: draftId }]);
    assert.deepEqual(h.calls.activeEndpoint, []);
    assert.equal(h.bag.activeTabId, draftId);
  });

  it('publishes the draft id for the tree to consume', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const draftId = await h.slice.createEndpointAt('m1');

    // Same shared signal as the dialog kinds. The difference is what the id
    // resolves to: a `draft:` id names a draft, not a row to rename — so
    // the tree has to recognise the prefix rather than look for a node.
    assert.equal(h.bag.lastCreatedId, draftId);
  });
});

describe('ui-new-item: a collection draft prefers its collection parent', () => {
  it('nests under the collection when the row it was opened from was one', async () => {
    const h = makeHarness([
      makeNode('m1', null, 'module'),
      makeNode('c1', 'm1', 'collection'),
      makeNode('c2', 'c1', 'collection'),
    ]);
    // "新建子集合" carries the *module* for context and the *collection*
    // for intent; the collection is the real parent.
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: 'c1' });

    const id = await h.slice.submitNewItem();

    assert.equal(id, 'node-new');
    assert.equal(h.calls.addNode[0].parentId, 'c1');
    assert.equal(h.calls.addNode[0].kind, 'collection');
  });

  it('falls back to the module when no collection was chosen', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: '' });

    await h.slice.submitNewItem();

    assert.equal(h.calls.addNode[0].parentId, 'm1');
  });

  it('refuses when neither a collection nor a module is chosen', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);
    h.bag.newItem = draft({ parentModuleId: '', parentCollectionId: '' });

    const id = await h.slice.submitNewItem();

    assert.equal(id, null, 'a node with no parent is not a tree');
    assert.equal(h.calls.addNode.length, 0, 'nothing may be written');
    assert.equal(h.bag.lastCreatedId, null);
  });
});

describe('ui-new-item: a module draft always lands at the project root', () => {
  it('writes a top-level module under the active project', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);
    h.bag.newItem = draft({ type: 'module', name: '  ' });

    const id = await h.slice.submitNewItem();

    assert.equal(id, 'node-new');
    const call = h.calls.addNode[0];
    assert.equal(call.parentId, null);
    assert.equal(call.rootId, 'p1');
    assert.equal(call.kind, 'module');
    // Blank name falls back to the kind's default rather than writing "".
    assert.equal(call.name, '新模块');
  });
});

describe('ui-new-item: changing the module clears the collection', () => {
  it('drops a collection that belongs to the previous module', () => {
    const h = makeHarness([makeNode('m1', null, 'module'), makeNode('c1', 'm1', 'collection')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: 'c1' });

    h.slice.setNewItemParentModule('m2');

    assert.equal(h.bag.newItem.parentModuleId, 'm2');
    // Leaving it set would create the collection under a collection of a
    // module the user has just moved away from.
    assert.equal(h.bag.newItem.parentCollectionId, '');
  });
});

describe('ui-new-item: a new project is entered, not just created', () => {
  it('calls openProject so the tree scopes to it and its card opens', async () => {
    const h = makeHarness([]);
    h.bag.newItem = draft({ type: 'project', name: '支付网关' });

    const id = await h.slice.submitNewItem();

    assert.equal(id, 'proj-1');
    assert.deepEqual(h.calls.addProject, [
      {
        name: '支付网关',
        opts: { description: '', color: '#000000' },
      },
    ]);
    // Not `setActiveProject`: the project is not a tree row any more, so
    // creating it without opening its tab would leave the user staring
    // at whatever pane they had before.
    assert.deepEqual(h.calls.activeProject, ['proj-1']);
    // No endpoint owns the new project yet — the last-response cache
    // from the previous project must not be handed to it.
    assert.deepEqual(h.calls.activeEndpoint, [null]);
  });
});
