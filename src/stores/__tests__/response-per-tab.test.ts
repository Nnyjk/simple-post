/**
 * 响应挂在标签上——`src/stores/slices/ui-response.ts` 与 `src/stores/slices/ui-tabs.ts`
 * 之间的契约。
 *
 * **为什么用真实组合 store。** 被测行为跨切片：响应状态住在 `ui-response`，而
 * 「标签没了就把它带走」这件事住在 `ui-tabs`（关标签、清扫失效标签、全关）与
 * `crud-tree` / `crud-project`（删接口、删项目）。stub 掉任何一侧，测的就不是
 * 同一件事了。
 *
 * 运行：npx tsx --test src/stores/__tests__/response-per-tab.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
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
const E2 = node({ id: 'e2', rootId: 'p1', parentId: 'm1', kind: 'endpoint', name: '查询' });

let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  origError = console.error.bind(console);
  console.error = () => {};

  // 删除动作要真的落到后端才谈得上「删掉了」——删除失败时 store 会回滚到快照，
  // 那份响应自然也还在。故这里放行 `nodes_delete`，其余命令一律视为不该发生。
  installIpcMock((cmd) => {
    if (cmd === 'nodes_delete') return undefined;
    throw new Error(`unexpected IPC during a response-scope test: ${cmd}`);
  });

  useAppStore.setState({
    projects: [],
    nodes: [M1, E1, E2],
    endpointDetails: {},
    nodesLoaded: true,
    activeProjectId: 'p1',
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
    lastResponses: {},
    responseHistory: {},
    responseExamples: {},
  });
});

afterEach(() => {
  console.error = origError;
  clearIpcMock();
});

/** 一份最小的响应。 */
const RESPONSE = {
  status: 200,
  statusText: 'OK',
  durationMs: 1,
  sizeBytes: 0,
  headers: {},
  body: '{"ok":true}',
  contentType: 'application/json',
  method: 'GET',
  url: 'http://x.test/y',
} as never;

describe('响应按标签存放', () => {
  it('两个标签各存各的，互不覆盖', () => {
    const store = useAppStore.getState();

    store.setLastResponse('e1', RESPONSE);
    store.setLastResponse('e2', { ...(RESPONSE as object), status: 500 } as never);

    const responses = useAppStore.getState().lastResponses;
    assert.equal(responses['e1']?.response, RESPONSE, 'e1 那份不被 e2 的写入换掉');
    assert.equal((responses['e2']?.response as { status: number }).status, 500);
  });

  it('同一个标签的第二次发送换掉上一次响应', () => {
    const store = useAppStore.getState();

    store.setLastResponse('e1', RESPONSE);
    store.setLastResponse('e1', { ...(RESPONSE as object), status: 404 } as never);

    const responses = useAppStore.getState().lastResponses;
    assert.equal((responses['e1']?.response as { status: number }).status, 404);
    assert.equal(Object.keys(responses).length, 1, '同一个标签不新增条目');
  });

  it('换响应就换掉脚本产出——回看历史时它并不存在', () => {
    const store = useAppStore.getState();

    store.setLastScriptOutcome('e1', { logs: [], assertions: [], errors: [], envWrites: [] });
    assert.ok(useAppStore.getState().lastResponses['e1']?.scriptOutcome);

    store.setLastResponse('e1', RESPONSE);
    assert.equal(useAppStore.getState().lastResponses['e1']?.scriptOutcome, null);
  });

  it('在途标记与响应是同一份状态，但彼此不互相清掉', () => {
    const store = useAppStore.getState();

    store.setRequestPending('e1', true);
    store.setLastResponse('e1', RESPONSE);

    const state = useAppStore.getState().lastResponses['e1'];
    assert.equal(state?.response, RESPONSE, '响应到达不把在途标记带走');
    assert.equal(state?.pending, true, '在途标记也不因为响应到达而消失');
  });
});

describe('切接口不再清空响应', () => {
  it('树里点接口不把它刚发出来的那次响应弄丢', () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.setLastResponse('e1', RESPONSE);
    // 重复点同一个接口：早先这里是 `setLastResponse(null)`，那份响应当场就没了。
    store.setActiveEndpoint('e1');

    assert.equal(useAppStore.getState().lastResponses['e1']?.response, RESPONSE);
  });

  it('从 A 切到 B，B 不会显示 A 的响应', () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.setLastResponse('e1', RESPONSE);

    store.setActiveEndpoint('e2');

    const responses = useAppStore.getState().lastResponses;
    assert.equal(responses['e2'], undefined, 'B 还没有自己的响应');
    assert.equal(responses['e1']?.response, RESPONSE, 'A 那份还在，没被 B 顶掉');
  });
});

describe('标签没了，它带走的响应也没了', () => {
  it('关标签回收那份响应', () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.setLastResponse('e1', RESPONSE);
    assert.ok(useAppStore.getState().lastResponses['e1']);

    store.closeTab('e1');

    assert.equal(useAppStore.getState().lastResponses['e1'], undefined);
  });

  it('关掉别的标签不影响这一份', () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.openTab('endpoint', 'e2');
    store.setLastResponse('e1', RESPONSE);

    store.closeTab('e2');

    assert.equal(useAppStore.getState().lastResponses['e1']?.response, RESPONSE);
  });

  it('关掉全部标签回收全部响应', () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.openTab('endpoint', 'e2');
    store.setLastResponse('e1', RESPONSE);
    store.setLastResponse('e2', RESPONSE);

    store.closeAllTabs();

    assert.deepEqual(useAppStore.getState().lastResponses, {});
  });

  it('清扫失效标签时一并回收它带走的响应', () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.setLastResponse('e1', RESPONSE);
    // 树里那个接口没了（别处的删除动作留下的状态），标签于是成为失效标签。
    useAppStore.setState({ nodes: [M1, E2] });

    store.pruneStaleTabs();

    assert.equal(useAppStore.getState().lastResponses['e1'], undefined);
  });
});

describe('删接口与删项目一并回收响应', () => {
  it('删掉接口，它那份响应不再留着', async () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.setLastResponse('e1', RESPONSE);

    await store.deleteNode('e1');

    assert.equal(useAppStore.getState().lastResponses['e1'], undefined);
  });

  it('删掉接口不影响别的接口的响应', async () => {
    const store = useAppStore.getState();

    store.openTab('endpoint', 'e1');
    store.openTab('endpoint', 'e2');
    store.setLastResponse('e1', RESPONSE);
    store.setLastResponse('e2', RESPONSE);

    await store.deleteNode('e1');

    const responses = useAppStore.getState().lastResponses;
    assert.equal(responses['e1'], undefined);
    assert.equal(responses['e2']?.response, RESPONSE);
  });
});
