/**
 * Slice-level test for `src/stores/slices/ui-new-item.ts` — the
 * "create a thing" modal's commit path.
 *
 * Target: the endpoint branch's parent choice. The parent/child matrix
 * gives an endpoint two legal parents, and this slice decides which one
 * from the draft:
 *
 *   - `parentCollectionId` set  → create inside that collection
 *   - `parentCollectionId` empty → create directly under `parentModuleId`
 *   - both empty                 → refuse (null), write nothing
 *
 * The middle case is the one the contract change added; the third is the
 * guard that must survive it, because an endpoint with no parent is not a
 * tree. The collection case is here as the unchanged control.
 *
 * Run via:
 *   npx tsx --test src/stores/__tests__/ui-new-item.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createUiNewItem } from '../slices/ui-new-item';
import type { NewItemDraft } from '../store-types';
import type { ResourceNode } from '@/types/domain';

/** What `addNode` was asked to create, captured for assertions. */
interface AddNodeCall {
  parentId: string | null;
  kind: string;
  name: string;
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
    type: 'endpoint',
    parentModuleId: '',
    parentCollectionId: '',
    name: 'n',
    description: '',
    baseUrl: '',
    color: '#000000',
    ...overrides,
  };
}

/**
 * A minimal store for the one action under test. `submitNewItem` only
 * reaches for the node list, the expand toggle, and `addNode` /
 * `setActiveEndpoint`, so everything else is a stub.
 */
function makeHarness(nodeList: ResourceNode[]) {
  const calls: {
    addNode: AddNodeCall[];
    expanded: string[];
    activeEndpoint: string[];
    activeProject: string[];
  } = {
    addNode: [],
    expanded: [],
    activeEndpoint: [],
    activeProject: [],
  };
  const bag = {
    nodes: nodeList,
    newItem: draft(),
    activeProjectId: 'p1',
    lastCreatedId: null as string | null,
    addProject: async () => 'proj-1',
    addNode: async (input: AddNodeCall) => {
      calls.addNode.push(input);
      return 'ep-new';
    },
    setActiveProject: () => {},
    openProject: (id: string) => calls.activeProject.push(id),
    setActiveEndpoint: (id: string) => calls.activeEndpoint.push(id),
    toggleNodeExpanded: (id: string) => calls.expanded.push(id),
  };
  const set = (partial: unknown) => {
    const updater =
      typeof partial === 'function' ? (partial as (state: typeof bag) => unknown)(bag) : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = createUiNewItem(set as never, get as never, {} as never);
  return { bag, calls, slice };
}

describe('ui-new-item: submitNewItem picks the endpoint parent', () => {
  it('creates under the module when no collection was chosen', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: '' });

    const id = await h.slice.submitNewItem();

    assert.equal(id, 'ep-new');
    assert.equal(h.calls.addNode.length, 1);
    const call = h.calls.addNode[0];
    assert.equal(call.parentId, 'm1', 'the module is the parent');
    assert.equal(call.kind, 'endpoint');
    // The detail row is what makes the new row openable.
    assert.deepEqual(call.detail, { url: '' });
  });

  it('still creates inside the collection when one was chosen', async () => {
    const h = makeHarness([makeNode('m1', null, 'module'), makeNode('c1', 'm1', 'collection')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: 'c1' });

    await h.slice.submitNewItem();

    assert.equal(h.calls.addNode[0].parentId, 'c1');
  });

  it('refuses when neither a collection nor a module is chosen', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);
    h.bag.newItem = draft({ parentModuleId: '', parentCollectionId: '' });

    const id = await h.slice.submitNewItem();

    assert.equal(id, null, 'an endpoint with no parent is not a tree');
    assert.equal(h.calls.addNode.length, 0, 'nothing may be written');
    assert.equal(h.bag.lastCreatedId, null);
  });

  it('expands the module it landed on so the new row is visible', async () => {
    const h = makeHarness([makeNode('m1', null, 'module', false)]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: '' });

    await h.slice.submitNewItem();

    assert.deepEqual(h.calls.expanded, ['m1']);
  });

  it('walks every ancestor of a chosen collection', async () => {
    const h = makeHarness([
      makeNode('m1', null, 'module', false),
      makeNode('c1', 'm1', 'collection', false),
      makeNode('c2', 'c1', 'collection', false),
    ]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: 'c2' });

    await h.slice.submitNewItem();

    // The collection itself, then its parent, then the module.
    assert.deepEqual(h.calls.expanded, ['c2', 'c1', 'm1']);
  });

  it('activates the new endpoint so the tester opens what was created', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: '' });

    await h.slice.submitNewItem();

    assert.deepEqual(h.calls.activeEndpoint, ['ep-new']);
    assert.equal(h.bag.lastCreatedId, 'ep-new');
  });
});

describe('ui-new-item: changing the module clears the collection', () => {
  it('drops a collection that belongs to the previous module', () => {
    const h = makeHarness([makeNode('m1', null, 'module'), makeNode('c1', 'm1', 'collection')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: 'c1' });

    h.slice.setNewItemParentModule('m2');

    assert.equal(h.bag.newItem.parentModuleId, 'm2');
    // Leaving it set would create the endpoint under a collection of a
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
    // Not `setActiveProject`: the project is not a tree row any more, so
    // creating it without opening its tab would leave the user staring
    // at whatever pane they had before.
    assert.deepEqual(h.calls.activeProject, ['proj-1']);
    // No endpoint owns the new project yet — the last-response cache
    // from the previous project must not be handed to it.
    assert.deepEqual(h.calls.activeEndpoint, [null]);
  });
});
