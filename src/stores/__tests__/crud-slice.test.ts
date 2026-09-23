/**
 * Slice-level test for `src/stores/crud-slice.ts`.
 *
 * Target from the second-pass audit (P2 tier):
 *   - T-2: Every CRUD slice action follows the same shape —
 *     optimistic local mutation via `set()`, fire-and-forget
 *     `invoke<...>(COMMANDS.*)`, on error rollback to the previous
 *     state via `set()`. We pick the `endpoint` family (`updateEndpoint`)
 *     because it's the most-called optimistic action in the tree and
 *     it exercises the optimistic+rollback branch with two distinct
 *     store fields (the endpoints array and `updatedAt`).
 *
 * Coverage:
 *   - Seeds an endpoint in the store.
 *   - Invokes `updateEndpoint` against an `invoke` mock that rejects.
 *   - Awaits the returned promise (the action awaits the IPC).
 *   - Asserts the rollback restored the original endpoint including
 *     its `updatedAt` timestamp and that a warning was logged.
 *
 * Run via:
 *   npx tsx --test src/stores/__tests__/crud-slice.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { createCrudSlice } from '../crud-slice';
import type { Endpoint } from '@/types/domain';

/** A minimal but domain-shaped `Endpoint`. */
function makeEndpoint(overrides: Partial<Endpoint> = {}): Endpoint {
  return {
    id: 'ep-1',
    collectionId: 'col-1',
    name: 'List Widgets',
    method: 'GET',
    url: '/widgets',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'none' },
    tags: [],
    sortOrder: 0,
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    ...overrides,
  };
}

/**
 * Build a "store" for the CRUD slice. The endpoint family only reads
 * `endpoints` out of state and writes back, so the harness can be
 * slimmer than for mcp-slice — we don't need other slice methods to
 * be present on the state object.
 */
function makeCrudHarness(initialEndpoints: Endpoint[]) {
  const bag: Record<string, any> = {
    endpoints: initialEndpoints,
    openTabs: [],
    activeTabId: null,
    activeEndpointId: null,
  };
  const set = (partial: any) => {
    const updater = typeof partial === 'function' ? partial(bag) : partial;
    if (updater) Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = createCrudSlice(set as any, get as any, {} as any);
  // The slice method `updateEndpoint` only reads `endpoints` out of
  // state; the slice itself doesn't call into other slice methods,
  // so a flat attachment isn't strictly needed. We still expose all
  // action methods for completeness.
  return { bag, slice };
}

let warnings: string[] = [];
let origWarn: typeof console.warn;
let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  // Spy on console.warn / console.error so we can assert the
  // rollback branch logged the IPC failure (production behaviour).
  origWarn = console.warn.bind(console);
  origError = console.error.bind(console);
  warnings = [];
  const fmt = (a: unknown) => {
    if (typeof a === 'string') return a;
    if (a instanceof Error) return `${a.name}: ${a.message}`;
    try {
      return JSON.stringify(a);
    } catch {
      return String(a);
    }
  };
  console.warn = (...args: unknown[]) => {
    warnings.push(args.map(fmt).join(' '));
  };
  console.error = (...args: unknown[]) => {
    warnings.push(args.map(fmt).join(' '));
  };
});

afterEach(() => {
  console.warn = origWarn;
  console.error = origError;
  clearIpcMock();
});

// ============================================================
// T-2 — optimistic update + rollback
// ============================================================

describe('crud-slice: updateEndpoint — T-2 optimistic + rollback', () => {
  it('applies optimistic update, then rolls back when invoke rejects', async () => {
    const original = makeEndpoint({
      id: 'ep-rollback',
      name: 'Original Name',
      url: '/widgets',
      updatedAt: 1_700_000_000_000,
    });
    const harness = makeCrudHarness([original]);
    const before = Date.now();

    // Mock `endpoints_update` to reject. The slice's catch branch should
    // fire the rollback set() and `console.error` the failure.
    installIpcMock(async (cmd) => {
      if (cmd === 'endpoints_update') {
        throw new Error('sqlite: disk I/O error');
      }
      throw new Error('unexpected cmd: ' + cmd);
    });

    // Snapshot pre-call state for the equality assertion below.
    const beforeUpdate = harness.bag.endpoints[0];
    assert.equal(beforeUpdate.name, 'Original Name');
    const originalUpdatedAt = beforeUpdate.updatedAt;

    await harness.slice.updateEndpoint('ep-rollback', {
      name: 'Renamed URL',
      url: '/widgets/v2',
    });

    const after = harness.bag.endpoints[0];
    // The original endpoint must still be in the store untouched.
    assert.ok(after, 'endpoint not deleted');
    assert.equal(after.id, 'ep-rollback', 'same id');
    assert.equal(
      after.name,
      'Original Name',
      'optimistic name change rolled back to "Original Name"',
    );
    assert.equal(after.url, '/widgets', 'optimistic url change rolled back');
    assert.equal(
      after.updatedAt,
      originalUpdatedAt,
      'updatedAt rolled back to the original timestamp',
    );

    // The slice should have emitted a console warning/error carrying
    // the same error message — that's how the prod UX surfaces the
    // failure. We accept either channel since the slice uses both
    // `console.warn` (for 'poll' style) and `console.error` (for
    // writes) — see CRUD slice header.
    const seenFailure = warnings.some(
      (w) => /sqlite.*disk I.O. error/i.test(w) || /Error: sqlite: disk I\/O error/i.test(w),
    );
    assert.ok(
      seenFailure,
      'failure should have been logged to console.warn|console.error; got: ' +
        JSON.stringify(warnings),
    );

    // Sanity: the recovered object references the very same `prev`
    // capture the slice held (or an object equal to it). Identity
    // would be too strong because the slice stores the optimistic
    // object back into the array, then replaces it with `prev` on
    // rollback — so what we should see IS `prev`.
    assert.strictEqual(
      after,
      beforeUpdate,
      'rolled-back endpoint reuses the prev snapshot reference',
    );
    void before; // (kept for documentation; tested implicitly via the
    // reference equality above — the rollback puts the
    // original `prev` straight back into the array.)
  });
});
