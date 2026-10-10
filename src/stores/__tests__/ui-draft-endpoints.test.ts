/**
 * Tests for `src/stores/slices/ui-draft-endpoints.ts` — the endpoint that
 * exists only in the renderer until the user presses Save.
 *
 * These run against the **real composed store** rather than a hand-built
 * harness, because the contract they protect is a contract between slices:
 * a draft writes nothing, `saveDraftEndpoint` hands the tab over to a real
 * `endpoint`, and `pruneStaleTabs` (a different sub-factory) must not read
 * a missing `nodes` row as "deleted". A harness would stub away exactly the
 * wiring most likely to break.
 *
 * "Writes nothing" is asserted two ways, because either alone is weak: the
 * IPC mock counts calls, and `nodes.length` is checked. A draft that
 * optimistically appended a local row would pass the first and fail the
 * second, and vice versa.
 *
 * Run via: npx tsx --test src/stores/__tests__/ui-draft-endpoints.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { DRAFT_ID_PREFIX, UNTITLED_ENDPOINT_NAME } from '../store-types';
import type { ResourceNode } from '@/types/domain';

const MODULE: ResourceNode = {
  id: 'm1',
  rootId: 'p1',
  parentId: null,
  kind: 'module',
  name: '支付',
  sortOrder: 0,
  createdAt: 0,
  updatedAt: 0,
  expanded: true,
};

const COLLECTION: ResourceNode = { ...MODULE, id: 'c1', parentId: 'm1', kind: 'collection' };

/** Every IPC call the store made, in order. The "did it write?" oracle. */
let ipcCalls: { cmd: string; args: Record<string, unknown> }[] = [];
/** Make `nodes_create` reject, the way a daemon failure would. */
let failCreate = false;
/** What the daemon hands back — deliberately NOT the id we offered it. */
const SERVER_ID = 'ep-server-1';

let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  ipcCalls = [];
  failCreate = false;
  origError = console.error.bind(console);
  console.error = () => {}; // addNode logs its own failure; keep output clean

  installIpcMock((cmd, args) => {
    ipcCalls.push({ cmd, args: args ?? {} });
    if (cmd === 'nodes_create') {
      if (failCreate) throw new Error('nodes_create failed');
      // Mirrors the real Tauri command, which mints its own id and ignores
      // the one we offered — so a test that keys off the offered id fails
      // here rather than only against a live daemon.
      return {
        id: SERVER_ID,
        rootId: 'p1',
        parentId: args.parentId ?? null,
        kind: args.kind,
        name: args.name,
        sortOrder: 0,
        createdAt: 0,
        updatedAt: 0,
      };
    }
    throw new Error(`unexpected command: ${cmd}`);
  });

  useAppStore.setState({
    nodes: [MODULE, COLLECTION],
    projects: [],
    endpointDetails: {},
    activeProjectId: 'p1',
    activeEndpointId: null,
    nodesLoaded: true,
    openTabs: [],
    activeTabId: null,
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

/** The tab showing `id`, or undefined — a bare reference, never an array. */
function tabFor(id: string) {
  return useAppStore.getState().openTabs.find((t) => t.id === id);
}

describe('draft endpoints: creating one touches neither the database nor the tree', () => {
  it('returns a prefixed id and writes nothing at all', () => {
    const before = useAppStore.getState().nodes.length;

    const draftId = useAppStore.getState().createEndpointDraft('m1');

    assert.ok(draftId.startsWith(DRAFT_ID_PREFIX), `expected a draft id, got ${draftId}`);
    assert.equal(useAppStore.getState().nodes.length, before, 'nodes must not grow');
    assert.equal(ipcCalls.length, 0, 'no command may reach the daemon');
  });

  it('opens and activates a tab of its own kind', () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');

    const state = useAppStore.getState();
    assert.equal(tabFor(draftId)?.kind, 'endpoint-draft');
    assert.equal(state.activeTabId, draftId);
    // Deliberately NOT `endpoint`: nothing in `nodes` answers to this id,
    // and every "does this row still exist?" sweep keys off the kind.
    assert.equal(state.openTabs.filter((t) => t.kind === 'endpoint').length, 0);
  });

  it('starts untitled, empty and bound to its parent', () => {
    const draftId = useAppStore.getState().createEndpointDraft('c1');

    const draft = useAppStore.getState().draftEndpoints[draftId];
    assert.equal(draft.parentId, 'c1');
    assert.equal(draft.rootId, 'p1');
    assert.equal(draft.name, UNTITLED_ENDPOINT_NAME);
    assert.equal(draft.method, 'GET');
    assert.equal(draft.url, '');
  });

  it('createEndpointAt mints a draft and publishes its id — still no write', async () => {
    const draftId = await useAppStore.getState().createEndpointAt('c1');

    assert.ok(draftId?.startsWith(DRAFT_ID_PREFIX));
    const state = useAppStore.getState();
    assert.equal(state.nodes.length, 2, 'the tree is unchanged');
    assert.equal(ipcCalls.length, 0);
    // The tree watches this to react to a new row; with a draft there is no
    // row, so the contract is "a draft appeared under this id".
    assert.equal(state.lastCreatedId, draftId);
    assert.equal(tabFor(draftId!)?.kind, 'endpoint-draft');
    // Not an endpoint activation: `setActiveEndpoint` would hand the
    // workspace a tab for a row that does not exist.
    assert.equal(state.activeEndpointId, null);
  });

  it('still refuses a parent that cannot hold an endpoint', async () => {
    assert.equal(await useAppStore.getState().createEndpointAt('nope'), null);
    assert.equal(await useAppStore.getState().createEndpointAt(''), null);
    assert.equal(Object.keys(useAppStore.getState().draftEndpoints).length, 0);
  });
});

describe('draft endpoints: editing one in place', () => {
  it('renames and re-points the url without touching the tree', () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');

    useAppStore.getState().updateDraftEndpoint(draftId, { name: '用户列表' });
    useAppStore.getState().updateDraftEndpoint(draftId, { url: '/users', method: 'POST' });

    const draft = useAppStore.getState().draftEndpoints[draftId];
    assert.equal(draft.name, '用户列表');
    assert.equal(draft.url, '/users');
    assert.equal(draft.method, 'POST');
    assert.equal(useAppStore.getState().nodes.length, 2);
    assert.equal(ipcCalls.length, 0);
  });

  it('ignores an unknown id instead of resurrecting one', () => {
    useAppStore.getState().createEndpointDraft('m1');

    useAppStore.getState().updateDraftEndpoint(`${DRAFT_ID_PREFIX}gone`, { name: 'x' });

    assert.equal(Object.keys(useAppStore.getState().draftEndpoints).length, 1);
  });

  it('discards the draft, its pending edits and its dirty flag', () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.setEndpointDraft(draftId, { body: { mode: 'json', content: '{}' } });

    store.discardDraftEndpoint(draftId);

    const state = useAppStore.getState();
    assert.equal(draftId in state.draftEndpoints, false);
    assert.equal(draftId in state.endpointDrafts, false);
    assert.equal(state.dirtyTabIds.has(draftId), false);
  });
});

describe('draft endpoints: Save is the only thing that writes', () => {
  it('persists the node, hands over the tab and forgets the draft', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.updateDraftEndpoint(draftId, { name: '用户列表', method: 'POST', url: '/from-draft' });
    // The request-config editors write here, exactly as they do for a
    // saved endpoint — including the url, which is why the patch wins.
    store.setEndpointDraft(draftId, {
      url: '/from-patch',
      params: [{ id: 'k1', key: 'page', value: '1', enabled: true }],
    });

    const endpointId = await useAppStore.getState().saveDraftEndpoint(draftId);

    assert.equal(endpointId, SERVER_ID, 'the daemon decides the id, not us');
    assert.equal(ipcCalls.length, 1);
    assert.equal(ipcCalls[0].cmd, 'nodes_create');
    assert.equal(ipcCalls[0].args.kind, 'endpoint');
    assert.equal(ipcCalls[0].args.parentId, 'm1');

    const state = useAppStore.getState();
    assert.equal(state.nodes.length, 3, 'the tree finally grows');
    assert.equal(state.nodes.find((n) => n.id === SERVER_ID)?.name, '用户列表');
    // Document fields and request-config fields in one detail write.
    const detail = state.endpointDetails[SERVER_ID];
    assert.equal(detail.method, 'POST');
    assert.equal(detail.url, '/from-patch', 'the newest edit of a field wins');
    assert.equal(detail.params.length, 1);

    // The draft is gone from every map, and the tab changed kind.
    assert.equal(draftId in state.draftEndpoints, false);
    assert.equal(draftId in state.endpointDrafts, false);
    assert.equal(tabFor(draftId), undefined, 'the draft tab is closed');
    assert.equal(tabFor(SERVER_ID)?.kind, 'endpoint');
    assert.equal(state.activeTabId, SERVER_ID);
  });

  it('lands the pending edits in the row rather than re-seeding a draft', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.setEndpointDraft(draftId, {
      headers: [{ id: 'h1', key: 'X-Key', value: 'k', enabled: true }],
    });

    await store.saveDraftEndpoint(draftId);

    const state = useAppStore.getState();
    // The edit is in the *row* — which is the only place it survives a
    // reload, and the only place the assertion belongs.
    assert.equal(state.endpointDetails[SERVER_ID]?.headers?.[0]?.key, 'X-Key');
    // …and nowhere else. Copying the pending edits across to the new id
    // would recreate exactly what Save exists to end: a `•` on a saved
    // tab, "关闭已保存" declining to close it, and a second Save that
    // writes nothing new.
    assert.equal(
      state.endpointDrafts[SERVER_ID],
      undefined,
      'a saved endpoint has no pending edit',
    );
    assert.equal(state.dirtyTabIds.has(SERVER_ID), false, 'and is not dirty');
  });

  it('falls back to the untitled name rather than persisting an empty one', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    useAppStore.getState().updateDraftEndpoint(draftId, { name: '   ' });

    await useAppStore.getState().saveDraftEndpoint(draftId);

    assert.equal(
      useAppStore.getState().nodes.find((n) => n.id === SERVER_ID)?.name,
      UNTITLED_ENDPOINT_NAME,
    );
  });

  it('keeps everything when the write fails, so the user can retry', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.updateDraftEndpoint(draftId, { name: '用户列表' });
    store.setEndpointDraft(draftId, { body: { mode: 'json', content: '{"a":1}' } });
    failCreate = true;

    const endpointId = await useAppStore.getState().saveDraftEndpoint(draftId);

    const state = useAppStore.getState();
    assert.equal(endpointId, null);
    // Nothing was thrown away: the point of a draft is that a failed save
    // costs the user nothing.
    assert.equal(state.nodes.length, 2, 'nothing partial was written');
    assert.equal(state.draftEndpoints[draftId]?.name, '用户列表');
    assert.equal(state.endpointDrafts[draftId]?.body?.content, '{"a":1}');
    assert.equal(tabFor(draftId)?.kind, 'endpoint-draft');
    assert.ok(state.draftSaveError, 'the failure must be visible, not silent');
  });

  it('clears a stale error on the next attempt', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    failCreate = true;
    await useAppStore.getState().saveDraftEndpoint(draftId);
    assert.ok(useAppStore.getState().draftSaveError);

    failCreate = false;
    await useAppStore.getState().saveDraftEndpoint(draftId);

    assert.equal(useAppStore.getState().draftSaveError, null);
  });

  it('returns null for a draft that is not there', async () => {
    assert.equal(await useAppStore.getState().saveDraftEndpoint(`${DRAFT_ID_PREFIX}gone`), null);
    assert.equal(ipcCalls.length, 0);
  });
});

describe('draft endpoints: the tab survives the sweeps that prune dead tabs', () => {
  it('pruneStaleTabs cannot mistake "not written yet" for "deleted"', () => {
    // A real endpoint alongside the draft: the sweep must keep *both*, so
    // the exemption cannot be "keep everything".
    useAppStore.setState({
      nodes: [...useAppStore.getState().nodes, { ...MODULE, id: 'ep-real', kind: 'endpoint' }],
    });
    useAppStore.getState().openTab('endpoint', 'ep-real');
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    useAppStore.getState().setEndpointDraft(draftId, { url: '/kept' });

    useAppStore.getState().pruneStaleTabs();

    const state = useAppStore.getState();
    assert.equal(tabFor(draftId)?.kind, 'endpoint-draft', 'the draft tab must survive');
    assert.equal(tabFor('ep-real')?.kind, 'endpoint', 'a real endpoint tab survives too');
    assert.equal(state.endpointDrafts[draftId]?.url, '/kept', 'its edits survive with it');
    assert.equal(state.activeTabId, draftId);
  });

  it('still closes a real tab whose row is gone', () => {
    // The counterpart: the exemption is scoped to drafts, not a blanket
    // "stop pruning".
    useAppStore.getState().openTab('endpoint', 'ep-deleted');

    useAppStore.getState().pruneStaleTabs();

    assert.equal(tabFor('ep-deleted'), undefined);
  });

  it('closing a draft tab reclaims its pending edits', () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.setEndpointDraft(draftId, { url: '/x' });

    store.closeTab(draftId);

    const state = useAppStore.getState();
    assert.equal(tabFor(draftId), undefined);
    assert.equal(draftId in state.endpointDrafts, false);
    assert.equal(state.dirtyTabIds.has(draftId), false);
  });

  it('closeSavedTabs closes clean saved tabs and never a draft', () => {
    // Two invariants, and the second one is the regression this guards:
    // a draft tab is dirty from birth — there is no row it could have been
    // "saved" into — so "关闭已保存" must leave it alone; and the sweeps
    // that rebuild `draftEndpoints` / `endpointDrafts` key off the
    // *surviving* tabs by kind, so closing one tab cannot drag a still-open
    // draft's edits away with it.
    const kept = useAppStore.getState().createEndpointDraft('m1');
    useAppStore.getState().setEndpointDraft(kept, { url: '/kept' });
    // A saved endpoint with nothing pending — the only kind of tab this
    // action is entitled to close.
    useAppStore.getState().openTab('endpoint', 'ep-clean');

    const closed = useAppStore.getState().closeSavedTabs();

    const state = useAppStore.getState();
    assert.equal(closed, 1, 'only the saved-and-untouched endpoint closes');
    assert.equal(tabFor('ep-clean'), undefined);
    assert.equal(tabFor(kept)?.kind, 'endpoint-draft', 'a draft is never "saved"');
    assert.equal(state.endpointDrafts[kept]?.url, '/kept', 'and keeps its request-config edits');
    assert.equal(state.draftEndpoints[kept]?.id, kept, 'and its document fields');
  });

  it('closing a draft tab reclaims the draft itself, not just its edits', () => {
    // `closeTab` used to drop only `endpointDrafts`, which left the
    // document fields under an id no tab referenced — a draft that had
    // outlived its tab and could no longer be saved or reached.
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    useAppStore.getState().updateDraftEndpoint(draftId, { name: '改名过' });

    useAppStore.getState().closeTab(draftId);

    const state = useAppStore.getState();
    assert.equal(draftId in state.draftEndpoints, false);
    assert.equal(draftId in state.endpointDrafts, false);
  });
});
