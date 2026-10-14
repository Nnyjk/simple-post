/**
 * `src/stores/slices/ui-tabs.ts` 的测试——具体是草稿 / 脏标记的耦合，
 * 整个"编辑在 Save 之前都算未保存"模型就架在这条不变量上。
 *
 * 这里保护的是：
 *   - **每一个**草稿字段都会把标签标脏，不只是 url / method。"是否为空"的判断
 *     一旦是手写的那份，给 `EndpointDraft` 加上 `body` 就会让 body 的编辑对脏点
 *     静默不可见——这正是本套件要防的那类 bug。
 *   - 草稿是**合并**，所以先编辑 body 再编辑 params 两个都会留下。
 *   - 重写同一个值是空操作（无抖动，无无限渲染）。
 *   - `clearEndpointDraft` / `null` 把标签变回干净。
 *
 * 运行：npx tsx --test src/stores/__tests__/ui-tabs-draft.test.ts
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

/** 最小 state 包——`setEndpointDraft` 碰不到别的任何东西。 */
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

/** harness 能看到的草稿键，让失败信息可读。 */
function draftKeys(bag: Bag, id = 'ep1'): string[] {
  return Object.keys((bag.endpointDrafts[id] ?? {}) as object).sort();
}

describe('ui-tabs: every request-config edit marks the tab dirty', () => {
  it('marks dirty for a body edit alone', () => {
    // 回归点：如果 `hasAny` 是手写的 url/method 判断，一次 body 编辑就会产出一个
    // 永远进不了 `dirtyTabIds` 的草稿。
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
    // 先编辑 body 再编辑 params 不能丢掉 body 那次——草稿是累积的未保存改动集合，
    // 不是最后一次。
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
  // 它住在 crud-helpers 里，却是两条保存路径都依赖的契约，所以就在这里、
  // 紧挨它所消费的草稿形状来断言。
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
