/**
 * `src/stores/slices/ui-draft-endpoints.ts` 的测试——那个在用户按下 Save 之前
 * 只存在于渲染侧的 endpoint。
 *
 * 它们跑在**真实组合 store** 上而不是手搭的 harness，因为要保护的契约是切片
 * 之间的契约：草稿什么都不写，`saveDraftEndpoint` 把标签移交给一个真正的
 * `endpoint`，而 `pruneStaleTabs`（另一个子工厂）绝不能把缺失的 `nodes` 行读成
 * "已删除"。harness 恰好会把最容易坏的那段接线 stub 掉。
 *
 * "什么都不写"用两种方式断言，因为任何一种单独都不够：IPC mock 数调用次数，
 * 再查 `nodes.length`。乐观追加了一行本地行的草稿会通过前者、失败于后者，
 * 反之亦然。
 *
 * 运行：npx tsx --test src/stores/__tests__/ui-draft-endpoints.test.ts
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

/** store 发出过的每次 IPC 调用，按顺序。"写没写？"的裁判。 */
let ipcCalls: { cmd: string; args: Record<string, unknown> }[] = [];
/** 让 `nodes_create` reject，就像守护进程失败那样。 */
let failCreate = false;
/** 守护进程回什么——刻意**不是**我们提供给它的那个 id。 */
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
      // 镜像真实的 Tauri 命令：它自己铸 id 并忽略我们提供的那个——所以依赖所提供 id
      // 的测试在这里就失败，而不是只对着活着的守护进程才失败。
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

/** 显示 `id` 的标签，或 undefined——一个裸引用，永远不是数组。 */
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
    // 刻意**不**是 `endpoint`：`nodes` 里谁也不认识这个 id，而每一处
    // "这一行还在吗？"的清扫都按 kind 作键。
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
    // 树盯着它以便对新一行做出反应；而草稿没有行，所以契约是
    // "这个 id 之下出现了一个草稿"。
    assert.equal(state.lastCreatedId, draftId);
    assert.equal(tabFor(draftId!)?.kind, 'endpoint-draft');
    // 不是激活 endpoint：`setActiveEndpoint` 会递给工作区一个并不存在的行的标签。
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
    // 请求配置编辑器写在这里，与它们对已存 endpoint 所做的完全一样——包括 url，
    // 这正是补丁胜过文档字段的原因。
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
    // 文档字段与请求配置字段合并进同一次 detail 写入。
    const detail = state.endpointDetails[SERVER_ID];
    assert.equal(detail.method, 'POST');
    assert.equal(detail.url, '/from-patch', 'the newest edit of a field wins');
    assert.equal(detail.params.length, 1);

    // 草稿从每张映射里都没了，标签也换了 kind。
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
    // 这次编辑在**行**里——那是它熬过重载的唯一地方，也是断言唯一该落的地方。
    assert.equal(state.endpointDetails[SERVER_ID]?.headers?.[0]?.key, 'X-Key');
    // ……而且别处也没有。把待保存的编辑复制到新 id 之下，恰恰会重建 Save 存在的
    // 意义所要终结的东西：一个已保存标签上的 `•`、"关闭已保存"不肯关它、
    // 以及第二次写不出新东西的 Save。
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
    // 什么都没被丢掉：草稿的意义就在于保存失败不花用户任何代价。
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

describe('draft endpoints: the endpoint TLS settings travel with the row', () => {
  it('keeps the endpoint own TLS settings and writes them with the row', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.setEndpointDraft(draftId, {
      tls: {
        skipVerify: true,
        caCertificatePath: 'C:\\certs\\ca.pem',
        clientCertificatePath: null,
        clientKeyPath: null,
      },
    });
    // 设置还在草稿里，什么都没落到数据库。
    assert.equal(ipcCalls.length, 0, 'no command may reach the daemon before Save');

    await store.saveDraftEndpoint(draftId);

    const detail = useAppStore.getState().endpointDetails[SERVER_ID];
    assert.deepEqual(detail?.tls, {
      skipVerify: true,
      caCertificatePath: 'C:\\certs\\ca.pem',
      clientCertificatePath: null,
      clientKeyPath: null,
    });
    // 写给后端的那一次请求里也带着它。
    assert.deepEqual((ipcCalls[0].args.detail as { tls?: unknown } | undefined)?.tls, {
      skipVerify: true,
      caCertificatePath: 'C:\\certs\\ca.pem',
      clientCertificatePath: null,
      clientKeyPath: null,
    });
  });

  it('writes null when the endpoint goes back to inheriting the global default', async () => {
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    const store = useAppStore.getState();
    store.setEndpointDraft(draftId, {
      tls: {
        skipVerify: true,
        caCertificatePath: null,
        clientCertificatePath: null,
        clientKeyPath: null,
      },
    });
    // 用户把「继承全局设置」打开：端点不再带自己那份设置。
    store.setEndpointDraft(draftId, { tls: null });

    await store.saveDraftEndpoint(draftId);

    assert.equal(
      useAppStore.getState().endpointDetails[SERVER_ID]?.tls,
      null,
      'null = 回到继承全局默认',
    );
    assert.equal(
      (ipcCalls[0].args.detail as { tls?: unknown } | undefined)?.tls,
      null,
      'null 必须写进补丁，否则已存的那份设置清不掉',
    );
  });
});

describe('draft endpoints: the tab survives the sweeps that prune dead tabs', () => {
  it('pruneStaleTabs cannot mistake "not written yet" for "deleted"', () => {
    // 旁边还有一个真实的 endpoint：清扫必须**两个都**留下，所以这条豁免
    // 不可能是"全都留着"。
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
    // 反面：豁免只限草稿，不是一句笼统的"别清扫了"。
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
    // 两条不变量，第二条正是这个守卫要防的回归：
    // 草稿标签一出生就是脏的——它根本没有一行能被"保存"进去——所以
    // "关闭已保存"必须绕开它；而重建 `draftEndpoints` / `endpointDrafts` 的
    // 那些清扫按 kind 对**幸存**的标签作键，所以关掉一个标签不会把另一个仍
    // 开着的草稿的编辑一起带走。
    const kept = useAppStore.getState().createEndpointDraft('m1');
    useAppStore.getState().setEndpointDraft(kept, { url: '/kept' });
    // 一个什么都没有待保存内容的已存 endpoint——这个动作唯一有资格关掉的标签种类。
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
    // `closeTab` 只删 `endpointDrafts` 时，会把文档字段留在一个没有任何标签引用的
    // id 之下——一个活得比标签还久、既存不下去也够不着的草稿。
    const draftId = useAppStore.getState().createEndpointDraft('m1');
    useAppStore.getState().updateDraftEndpoint(draftId, { name: '改名过' });

    useAppStore.getState().closeTab(draftId);

    const state = useAppStore.getState();
    assert.equal(draftId in state.draftEndpoints, false);
    assert.equal(draftId in state.endpointDrafts, false);
  });
});
