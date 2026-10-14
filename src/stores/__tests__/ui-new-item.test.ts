/**
 * `src/stores/slices/ui-new-item.ts` 的切片级测试——创建路径的父节点选择。
 *
 * 覆盖两个入口：
 *
 *   - `createEndpointAt(parentId)` —— 不用对话框，父节点显式给出。endpoint
 *     现在就诞生于此，作为前端草稿：在 `saveDraftEndpoint` 之前什么都不会触及
 *     SQLite，所以这里的断言关于**不**写入，而不是关于写了什么。
 *   - `submitNewItem()` —— 三种对话框种类（project / module / collection）。
 *
 * 父子契约给了 endpoint 两个合法父节点（module 或 collection），collection 也
 * 同样是这两个，所以两条路径都必须从某处解析出父节点，并在解析不出时拒绝：
 *
 *   - `createEndpointAt`：拿到的 id，对着 `nodes` 校验
 *   - `submitNewItem`：`parentCollectionId || parentModuleId`，同样校验
 *
 * "拒绝"这个分支是必须活下来的守卫，因为没有父节点的节点不构成一棵树——对
 * endpoint 更是如此，因为父节点从未存在过的草稿永远存不下去。
 *
 * 运行：
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
 * 被测两个动作用的最小 store。它们只摸节点列表、展开开关、
 * `addNode` / `addProject` / `setActiveEndpoint` / `openProject`，其余都是桩。
 *
 * `createUiDraftEndpoints` 是真的组合进来的而不是打桩：`createEndpointAt` 会委托
 * 给 `createEndpointDraft`，在那里打桩测到的只是"调用发生过"，而不是"草稿出来了"。
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
  // 在真实 store 里动作是通过 `get()` 够到兄弟动作的，所以组合出来的动作必须挂在
  // `get` 返回的那个对象上，而不是摆在旁边。
  // beside it.
  Object.assign(bag, slice);
  return { bag, calls, slice };
}

describe('ui-new-item: createEndpointAt takes the parent from its argument', () => {
  it('creates an untitled draft under the module that was clicked, writing nothing', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const draftId = await h.slice.createEndpointAt('m1');

    // 读起来是暂定的，而不是一个最终名字。
    assert.ok(draftId, 'a valid parent must yield a draft id');
    const draft = h.bag.draftEndpoints[draftId] as { name: string; parentId: string };
    assert.equal(draft.name, UNTITLED_ENDPOINT_NAME);
    assert.equal(draft.parentId, 'm1');
    // 契约：没有行，其后也没有守护进程调用。Save 之前树得不到任何东西。
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
    // project 不是树节点，但这里只按 `kind` 判断，所以将来的节点类型默认被拒绝而不是
    // 被接受。
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

    // 集合本身，然后是它的父节点，然后是 module。
    assert.deepEqual(h.calls.expanded, ['c2', 'c1', 'm1']);
  });

  it('opens a draft tab — not an endpoint tab, which has no row to show', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const draftId = await h.slice.createEndpointAt('m1');

    // `setActiveEndpoint` 会为一个 `nodes` 里谁也不认识的 id 打开 `endpoint` 标签，
    // 所以草稿改为打开自己那种。
    assert.deepEqual(h.calls.openTab, [{ kind: 'endpoint-draft', refId: draftId }]);
    assert.deepEqual(h.calls.activeEndpoint, []);
    assert.equal(h.bag.activeTabId, draftId);
  });

  it('publishes the draft id for the tree to consume', async () => {
    const h = makeHarness([makeNode('m1', null, 'module')]);

    const draftId = await h.slice.createEndpointAt('m1');

    // 与各对话框种类共享同一个信号。区别在于这个 id 解析成什么：`draft:` id 指的是
    // 一个草稿，而不是一行待重命名的东西——所以树必须认得这个前缀，而不是去找节点。
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
    // 「新建子集合」带着*上下文用的 module*和*表达意图的 collection*；
    // collection 才是真正的父节点。
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
    // 名字留空时退回该种类的默认值，而不是写入 ""。
    assert.equal(call.name, '新模块');
  });
});

describe('ui-new-item: changing the module clears the collection', () => {
  it('drops a collection that belongs to the previous module', () => {
    const h = makeHarness([makeNode('m1', null, 'module'), makeNode('c1', 'm1', 'collection')]);
    h.bag.newItem = draft({ parentModuleId: 'm1', parentCollectionId: 'c1' });

    h.slice.setNewItemParentModule('m2');

    assert.equal(h.bag.newItem.parentModuleId, 'm2');
    // 不清掉的话，集合会被建在用户刚离开的某个 module 的集合之下。
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
    // 不是 `setActiveProject`：项目已经不再是树里的一行，所以建完却不打开它的标签，
    // 会让用户盯着切换前那个面板。
    assert.deepEqual(h.calls.activeProject, ['proj-1']);
    // 还没有 endpoint 属于这个新项目——上一个项目留下的最近响应缓存
    // 不能被交给它。
    assert.deepEqual(h.calls.activeEndpoint, [null]);
  });
});
