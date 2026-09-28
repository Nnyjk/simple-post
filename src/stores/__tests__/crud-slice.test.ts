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
import type { Endpoint, Environment } from '@/types/domain';

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

/** 用于环境值增删 action 的最小环境数据。 */
function makeEnvironment(overrides: Partial<Environment> = {}): Environment {
  return {
    id: 'env-1',
    projectId: 'project-1',
    name: 'dev',
    baseUrls: {},
    variables: {},
    isActive: true,
    ...overrides,
  };
}

/**
 * 构造 CRUD slice 所需的最小 store；环境 action 读取 environments，
 * endpoint action 读取 endpoints，二者共用 Zustand 风格的 setter。
 */
type CrudHarnessState = {
  endpoints: Endpoint[];
  environments: Environment[];
  openTabs: never[];
  activeTabId: null;
  activeEndpointId: null;
};

function makeCrudHarness(initialEndpoints: Endpoint[], initialEnvironments: Environment[] = []) {
  const bag: CrudHarnessState = {
    endpoints: initialEndpoints,
    environments: initialEnvironments,
    openTabs: [],
    activeTabId: null,
    activeEndpointId: null,
  };
  const set = (partial: unknown) => {
    const updater =
      typeof partial === 'function'
        ? (partial as (state: CrudHarnessState) => unknown)(bag)
        : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = createCrudSlice(set as never, get as never, {} as never);
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

type CapturedInvoke = { cmd: string; args: Record<string, unknown> };

describe('crud-slice: 环境值的 null 删除契约', () => {
  it('变量删除立即移除本地键，发送 null，并在成功路径保留空字符串值', async () => {
    const original = makeEnvironment({
      variables: { token: 'secret', empty: '' },
    });
    const harness = makeCrudHarness([], [original]);
    const calls: CapturedInvoke[] = [];
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });

    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      await pending;
      return null;
    });

    const action = harness.slice.setEnvironmentVar('env-1', 'token', null);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      { empty: '' },
      'null 应在 IPC 完成前从乐观状态移除变量',
    );

    release();
    await action;
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'token', value: null },
      },
    ]);

    await harness.slice.setEnvironmentVar('env-1', '', '');
    assert.deepEqual(
      harness.bag.environments[0].variables,
      { empty: '', '': '' },
      '空字符串是真实变量值，不应被当作删除',
    );
  });

  it('变量删除失败时恢复原变量和值', async () => {
    const original = makeEnvironment({ variables: { token: 'secret', keep: 'yes' } });
    const harness = makeCrudHarness([], [original]);
    installIpcMock(async () => {
      throw new Error('variable delete failed');
    });

    await harness.slice.setEnvironmentVar('env-1', 'token', null);

    assert.deepEqual(harness.bag.environments[0].variables, original.variables);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('BaseUrl null 删除槽位并发送 null，空 URL 仍作为真实值保留', async () => {
    const original = makeEnvironment({
      baseUrls: { api: 'https://api.example.test', empty: '' },
    });
    const harness = makeCrudHarness([], [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.setEnvironmentBaseUrl('env-1', 'api', null);
    assert.equal(
      Object.hasOwn(harness.bag.environments[0].baseUrls, 'api'),
      false,
      'null 应从乐观状态移除 BaseUrl 槽位',
    );
    assert.equal(harness.bag.environments[0].baseUrls.empty, '');

    await harness.slice.setEnvironmentBaseUrl('env-1', 'empty', '');
    assert.equal(harness.bag.environments[0].baseUrls.empty, '');
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_baseurl',
        args: { envId: 'env-1', defId: 'api', url: null },
      },
      {
        cmd: 'environments_set_baseurl',
        args: { envId: 'env-1', defId: 'empty', url: '' },
      },
    ]);
  });

  it('BaseUrl 删除失败时恢复原槽位和值', async () => {
    const original = makeEnvironment({ baseUrls: { api: 'https://api.example.test' } });
    const harness = makeCrudHarness([], [original]);
    installIpcMock(async () => {
      throw new Error('base URL delete failed');
    });

    await harness.slice.setEnvironmentBaseUrl('env-1', 'api', null);

    assert.deepEqual(harness.bag.environments[0].baseUrls, original.baseUrls);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('重命名按删除旧 key 后写入新 key 的顺序发送 null 与新值', async () => {
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness([], [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    const removeOld = harness.slice.setEnvironmentVar('env-1', 'old', null);
    const writeNew = harness.slice.setEnvironmentVar('env-1', 'renamed', 'value');
    await Promise.all([removeOld, writeNew]);

    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'old', value: null },
      },
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'renamed', value: 'value' },
      },
    ]);
    assert.deepEqual(harness.bag.environments[0].variables, {
      renamed: 'value',
      keep: 'yes',
    });
  });

  it('重命名中的删除 action 失败时恢复删除前的快照，不宣称 rename 原子性', async () => {
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness([], [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (args?.value === null) throw new Error('rename delete failed');
      return null;
    });

    const removeOld = harness.slice.setEnvironmentVar('env-1', 'old', null);
    const writeNew = harness.slice.setEnvironmentVar('env-1', 'renamed', 'value');
    await Promise.all([removeOld, writeNew]);

    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'old', value: null },
      },
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'renamed', value: 'value' },
      },
    ]);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      original.variables,
      '当前两次 action 的删除失败分支应恢复删除前快照',
    );
  });
});
