/**
 * 标签条的批量关闭动作 —— `src/stores/dirty-tabs-slice.ts` 的 `requestCloseOthers` /
 * `requestCloseLeft` / `requestCloseRight`，以及提示栏的 `cancel` 结算动作。
 *
 * 这里保护的是三件事：
 *
 *   - **「关闭其他」以被右键的那个标签为基准**，不是活动标签。标签条末尾那个「关闭其他标签」
 *     按钮走的是同一个动作但不传基准（保留活动标签），两条路径不能互相覆盖。
 *   - **「关闭左侧」/「关闭右侧」按标签条上的位置划分**，关掉的那一侧不含被点的标签本身。
 *   - **关掉提示栏等于取消**：脏标签一个都不关，未保存的修改原样留着。
 *
 * 跑在**真实组合 store** 上 —— 被测行为是切片之间的契约（`dirty-tabs-slice` 调用
 * `ui-tabs` 的 `closeTab`，而 `closeTab` 又要维护草稿与响应），stub 掉任何一侧都会把
 * 最容易坏的那段接线 stub 掉。
 *
 * 运行：npx tsx --test src/stores/__tests__/tab-close-actions.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';

let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  origError = console.error.bind(console);
  console.error = () => {};
  installIpcMock(() => null);

  useAppStore.setState({
    projects: [],
    nodes: [],
    endpointDetails: {},
    nodesLoaded: true,
    activeProjectId: '',
    activeEndpointId: null,
    openTabs: [],
    activeTabId: null,
    dirtyTabIds: new Set<string>(),
    endpointDrafts: {},
    draftEndpoints: {},
    lastResponses: {},
  });
});

afterEach(() => {
  console.error = origError;
  clearIpcMock();
});

/** 打开若干个标签，并把其中一个标脏（模拟有一份未保存的修改）。 */
function openTabs(ids: string[], dirtyId?: string) {
  useAppStore.setState({
    openTabs: ids.map((id) => ({ id, kind: 'endpoint' as const })),
    activeTabId: ids[0] ?? null,
    dirtyTabIds: new Set(dirtyId ? [dirtyId] : []),
  });
}

const tabIds = () => useAppStore.getState().openTabs.map((t) => t.id);

describe('批量关闭：关闭其他 / 关闭左侧 / 关闭右侧', () => {
  it('「关闭其他」保留被指定的那个标签，哪怕它不是活动标签', () => {
    openTabs(['t1', 't2', 't3']);
    useAppStore.setState({ activeTabId: 't2' });

    useAppStore.getState().requestCloseOthers('t3');

    assert.deepEqual(tabIds(), ['t3'], '活动标签 t2 也被关掉了 —— 基准是 t3');
  });

  it('不传基准时保留活动标签（标签条末尾那个按钮走的就是这条）', () => {
    openTabs(['t1', 't2', 't3']);
    useAppStore.setState({ activeTabId: 't2' });

    useAppStore.getState().requestCloseOthers();

    assert.deepEqual(tabIds(), ['t2']);
  });

  it('「关闭左侧」只关掉左边那些，被点的标签自己留着', () => {
    openTabs(['t1', 't2', 't3']);

    useAppStore.getState().requestCloseLeft('t2');

    assert.deepEqual(tabIds(), ['t2', 't3']);
  });

  it('「关闭右侧」只关掉右边那些', () => {
    openTabs(['t1', 't2', 't3']);

    useAppStore.getState().requestCloseRight('t2');

    assert.deepEqual(tabIds(), ['t1', 't2']);
  });

  it('已经在最左 / 最右的标签，那一侧没有可关的', () => {
    openTabs(['t1', 't2']);

    useAppStore.getState().requestCloseLeft('t1');
    assert.deepEqual(tabIds(), ['t1', 't2'], '左边没有标签');

    useAppStore.getState().requestCloseRight('t2');
    assert.deepEqual(tabIds(), ['t1', 't2'], '右边没有标签');
  });

  it('关掉的那一侧有脏标签时，它进队列而不是被直接关掉', () => {
    openTabs(['t1', 't2', 't3'], 't1');

    useAppStore.getState().requestCloseLeft('t3');

    const state = useAppStore.getState();
    assert.deepEqual(state.pendingDirtyClose?.tabIds, ['t1'], '脏标签排进队列');
    assert.ok(tabIds().includes('t1'), '脏标签还没被关掉');
    assert.ok(!tabIds().includes('t2'), '干净的已经关掉了');
  });
});

describe('关掉提示栏等于取消', () => {
  it('脏标签既没有被关闭，未保存的修改也还在', () => {
    openTabs(['t1', 't2'], 't1');
    useAppStore.getState().setEndpointDraft('t1', { url: '/未保存的路径' });

    useAppStore.getState().requestCloseTab('t1');
    assert.ok(useAppStore.getState().pendingDirtyClose, '提示栏打开了');

    useAppStore.getState().resolveDirtyClose('cancel');

    const state = useAppStore.getState();
    assert.equal(state.pendingDirtyClose, null, '批次清掉了');
    assert.ok(tabIds().includes('t1'), '标签还在');
    assert.equal(state.endpointDrafts.t1?.url, '/未保存的路径', '未保存的修改原样留着');
  });

  it('取消之后队列里剩下的标签也不再被关掉', () => {
    openTabs(['t1', 't2'], 't2');

    useAppStore.getState().requestCloseTab('t2');
    useAppStore.getState().resolveDirtyClose('cancel');

    assert.ok(tabIds().includes('t2'), '标签还在');
  });
});
