/**
 * Tests for `src/stores/slices/ui-tabs.ts` — specifically the draft /
 * dirty-flag coupling, which is the invariant the whole "edits are unsaved
 * until Save" model rests on.
 *
 * What is protected here:
 *   - **Every** draft field marks the tab dirty, not just url/method. The
 *     emptiness check used to be `url !== undefined || method !== undefined`
 *     written out by hand, so adding `body` to `EndpointDraft` would have
 *     silently left body edits invisible to the dirty dot — the exact bug
 *     this suite is here to prevent.
 *   - A draft is a *merge*, so editing body then params keeps both.
 *   - Re-writing the same value is a no-op (no churn, no infinite render).
 *   - `clearEndpointDraft` / `null` returns the tab to clean.
 *
 * Run via: npx tsx --test src/stores/__tests__/ui-tabs-draft.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createUiTabs } from '../slices/ui-tabs';
import type { AppState } from '../app-store';
import type { EndpointDetail, KeyValue, RequestBody } from '@/types/domain';

type SetState = (partial: unknown) => void;

interface Bag {
  openTabs: { id: string; kind: string }[];
  activeTabId: string | null;
  dirtyTabIds: Set<string>;
  endpointDrafts: Record<string, unknown>;
  activeSettingsSection: string;
}

/** Minimal state bag — `setEndpointDraft` touches nothing else. */
function makeHarness() {
  const bag: Bag = {
    openTabs: [{ id: 'ep1', kind: 'endpoint' }],
    activeTabId: 'ep1',
    dirtyTabIds: new Set<string>(),
    endpointDrafts: {},
    activeSettingsSection: 'env',
  };
  const set: SetState = (partial) => {
    const updater = typeof partial === 'function' ? (partial as (s: Bag) => unknown)(bag) : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const slice = createUiTabs(set as never, getStub as never, {} as never);
  function getStub(): AppState {
    return bag as unknown as AppState;
  }
  return { bag, slice };
}

const kv = (key: string, value = 'v'): KeyValue => ({
  id: `k-${key}`,
  key,
  value,
  enabled: true,
});
const jsonBody = (content: string): RequestBody => ({ mode: 'json', content });

/** Draft keys the harness can see, for readable failure messages. */
function draftKeys(bag: Bag, id = 'ep1'): string[] {
  return Object.keys((bag.endpointDrafts[id] ?? {}) as object).sort();
}

describe('ui-tabs: every request-config edit marks the tab dirty', () => {
  it('marks dirty for a body edit alone', () => {
    // The regression: `hasAny` used to be hand-written over url/method, so
    // a body edit produced a draft that never reached `dirtyTabIds`.
    const h = makeHarness();
    h.slice.setEndpointDraft('ep1', { body: jsonBody('{"a":1}') });
    assert.equal(h.bag.dirtyTabIds.has('ep1'), true);
    assert.deepEqual(draftKeys(h.bag), ['body']);
  });

  it('marks dirty for params, headers, auth, docs and tags alike', () => {
    for (const patch of [
      { params: [kv('a')] },
      { headers: [kv('X-Test')] },
      { auth: { kind: 'bearer' as const, bearer: 't' } },
      { docs: '# hi' },
      { tags: ['smoke'] },
    ]) {
      const h = makeHarness();
      h.slice.setEndpointDraft('ep1', patch);
      assert.equal(
        h.bag.dirtyTabIds.has('ep1'),
        true,
        `expected dirty for ${Object.keys(patch).join(',')}`,
      );
    }
  });

  it('merges patches instead of replacing them', () => {
    // Editing body then params must not drop the body edit — the draft is
    // the accumulated set of unsaved changes, not the last one.
    const h = makeHarness();
    h.slice.setEndpointDraft('ep1', { url: '/a' });
    h.slice.setEndpointDraft('ep1', { body: jsonBody('{}') });
    h.slice.setEndpointDraft('ep1', { tags: ['x'] });
    assert.deepEqual(draftKeys(h.bag), ['body', 'tags', 'url']);
  });

  it('treats re-writing an identical value as a no-op', () => {
    const h = makeHarness();
    const body = jsonBody('{"a":1}');
    h.slice.setEndpointDraft('ep1', { body });
    const draftsRef = h.bag.endpointDrafts;
    const dirtyRef = h.bag.dirtyTabIds;
    h.slice.setEndpointDraft('ep1', { body });
    assert.equal(h.bag.endpointDrafts, draftsRef, 'draft map identity should not change');
    assert.equal(h.bag.dirtyTabIds, dirtyRef, 'dirty set identity should not change');
  });

  it('clears both the draft and the dirty flag', () => {
    const h = makeHarness();
    h.slice.setEndpointDraft('ep1', { body: jsonBody('{}') });
    h.slice.clearEndpointDraft('ep1');
    assert.equal(h.bag.dirtyTabIds.has('ep1'), false);
    assert.equal('ep1' in h.bag.endpointDrafts, false);
  });

  it('an explicit null clears the draft', () => {
    const h = makeHarness();
    h.slice.setEndpointDraft('ep1', { body: jsonBody('{}') });
    h.slice.setEndpointDraft('ep1', null);
    assert.equal(h.bag.dirtyTabIds.has('ep1'), false);
  });
});

describe('draftToPatch — the shared draft → persistence mapping', () => {
  // Lives in crud-helpers but is the contract both save paths depend on,
  // so it is asserted here next to the draft shape it consumes.
  it('maps every defined draft field and drops undefined ones', async () => {
    const { draftToPatch } = await import('../slices/crud-helpers');
    const draft = {
      url: '/x',
      method: 'POST' as const,
      params: [kv('a')],
      body: jsonBody('{}'),
      docs: '# d',
      tags: ['t'],
      auth: undefined as unknown as EndpointDetail['auth'],
    };
    const patch = draftToPatch(draft);
    assert.deepEqual(patch, {
      url: '/x',
      method: 'POST',
      params: [kv('a')],
      body: jsonBody('{}'),
      docs: '# d',
      tags: ['t'],
    });
    assert.equal('auth' in patch, false, 'an undefined field must not be written');
  });
});
