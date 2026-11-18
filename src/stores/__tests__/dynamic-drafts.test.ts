/**
 * 动态变量草稿的存放位置与清理时机 —— `src/stores/dynamic-slice.ts` 的
 * `beginCreateDynamicVariableDraft` / `beginEditDynamicVariableDraft` /
 * `patchDynamicVariableDraft` / `clearDynamicVariableDraft`，以及「标签真被关掉」的那几处钩子
 * （`closeTab` / `pruneStaleTabs` / `closeAllTabs` / `closeSavedTabs`，以及删项目成功之后）。
 *
 * 保护的核心是「切到别的标签再切回来，编辑中的表单不能丢」：动态变量面板是项目内的一页，
 * `WorkspaceShell` 只挂载活动标签，切走就卸载。面板卸载在测试里的等价物是「没人再动它」——
 * 只要 store 里那份还在、引用还没换，重新挂载就能原样读回。故这里一条断言就是「组件状态
 * 不存在时草稿仍在」。
 *
 * 跑在**真实组合 store** 上：清理是切片之间的契约（ui-tabs 关标签 → 动态变量草稿），stub 掉
 * 任何一侧都会把最容易坏的那段接线 stub 掉。
 *
 * 运行：npx tsx --test src/stores/__tests__/dynamic-drafts.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { dynamicTabId } from '../store-types';
import type { DynamicVariableView, Project } from '@/types/domain';

function project(id: string): Project {
  return { id, name: id, color: '#000000', createdAt: 0, updatedAt: 0 };
}

/** 一条定义。表单从它垫出编辑草稿，故字段取默认值即可。 */
function view(over: Partial<DynamicVariableView> = {}): DynamicVariableView {
  return {
    id: 'd-sns',
    projectId: 'p1',
    name: 'sns',
    sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
    extractPath: '$.data',
    valueMode: 'auto',
    ttlSeconds: 60,
    staleFallback: false,
    enabled: true,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    source: 'auto',
    isOverride: false,
    ...over,
  };
}

const draftOf = (projectId: string) => useAppStore.getState().dynamicDrafts[projectId];

/** 当前 store 上的动作。每次现取：重置状态不影响动作。 */
const actions = () => useAppStore.getState();

let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  origError = console.error.bind(console);
  console.error = () => {};

  installIpcMock((cmd) => {
    // 列表命令一次性带回「定义 + 当前值 + 缓存状态」，这里只关心「有没有」。
    if (cmd === 'dynamic_variables_list') return [];
    return null;
  });

  useAppStore.setState({
    projects: [project('p1'), project('p2')],
    nodes: [],
    endpointDetails: {},
    nodesLoaded: true,
    activeProjectId: 'p1',
    activeEnvironmentId: '',
    openTabs: [],
    activeTabId: null,
    dirtyTabIds: new Set<string>(),
    endpointDrafts: {},
    draftEndpoints: {},
    projectTabs: {},
    lastResponses: {},
    lastStreams: {},
    dynamicVariables: [],
    dynamicDrafts: {},
    dynamicValues: {},
    dynamicErrors: {},
    dynamicInFlight: 0,
    savingDynamicVariableIds: new Set<string>(),
    dynamicError: null,
  });
});

afterEach(() => {
  console.error = origError;
  clearIpcMock();
});

describe('草稿留在 store 里：切标签不丢', () => {
  it('切到别的标签再切回来，草稿（含内容）原样还在', () => {
    const { beginCreateDynamicVariableDraft, patchDynamicVariableDraft } = actions();
    beginCreateDynamicVariableDraft('p1');
    patchDynamicVariableDraft('p1', {
      name: 'sns',
      extractPath: '$.data.list',
      sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
    });
    const before = draftOf('p1');

    // 切走：面板被卸载，组件的 useState 随之蒸发；store 里这份不受影响。
    useAppStore.setState({ activeTabId: 'some-other-tab' });
    // 切回来：面板重挂载，读到的还是同一个草稿对象。
    useAppStore.setState({ activeTabId: dynamicTabId('p1') });

    const after = draftOf('p1');
    assert.equal(after, before, '切标签不许换掉草稿对象（选择器靠引用比较）');
    assert.equal(after?.name, 'sns');
    assert.equal(after?.extractPath, '$.data.list');
    assert.equal(after?.creating, true);
  });

  it('两个项目的草稿互不串，改一份不动另一份的引用', () => {
    const { beginCreateDynamicVariableDraft, patchDynamicVariableDraft } = actions();
    beginCreateDynamicVariableDraft('p1');
    patchDynamicVariableDraft('p1', { name: 'sns' });
    beginCreateDynamicVariableDraft('p2');
    patchDynamicVariableDraft('p2', { name: 'points' });

    const p2Before = draftOf('p2');
    patchDynamicVariableDraft('p1', { name: 'sns_v2' });

    assert.equal(draftOf('p1')?.name, 'sns_v2');
    assert.equal(draftOf('p2')?.name, 'points', '改 p1 的草稿不许写进 p2');
    assert.equal(draftOf('p2'), p2Before, '改 p1 的草稿不许换掉 p2 的对象');
  });

  it('项目还没有草稿时打一格 patch：草稿记下来，但不会凭空开出一张新建表', () => {
    actions().patchDynamicVariableDraft('p2', { name: 'typed' });

    const draft = draftOf('p2');
    assert.equal(draft?.name, 'typed');
    assert.equal(draft?.creating, false, '没人开表单，patch 不许把它变成「正在新建」');
    assert.equal(draft?.editingId, null, '也不属于任何一条定义');
    assert.equal(draftOf('p1'), undefined);
  });

  it('编辑草稿按定义垫出全部字段；再次打开同一条不重置用户改过的内容', () => {
    const definition = view();
    useAppStore.setState({ dynamicVariables: [definition] });

    actions().beginEditDynamicVariableDraft(definition);
    const seeded = draftOf('p1');
    assert.equal(seeded?.editingId, 'd-sns');
    assert.equal(seeded?.creating, false);
    assert.equal(seeded?.name, 'sns');
    assert.equal(seeded?.description, '');
    assert.equal(seeded?.sourceKind, 'inline');
    assert.equal(seeded?.sourceEndpointId, '');
    assert.equal(seeded?.sourceProjectId, '');
    assert.equal(seeded?.sourceEndpointName, '');
    assert.equal(seeded?.environmentId, '');
    assert.equal(seeded?.extractPath, '$.data');
    assert.equal(seeded?.transformScript, '');
    assert.equal(seeded?.valueMode, 'auto');
    assert.equal(seeded?.ttlSeconds, '60', '缓存时长按输入框的原文存，才能留住「空」');
    assert.equal(seeded?.staleFallback, false);
    assert.equal(seeded?.enabled, true);

    actions().patchDynamicVariableDraft('p1', { name: 'sns_v2' });
    actions().beginEditDynamicVariableDraft(definition);
    assert.equal(
      draftOf('p1')?.name,
      'sns_v2',
      '正在编辑的就是这一条：不许把改了一半的名字重置回去',
    );
  });

  it('数据源接口被删过的定义落在「引用接口」形态，好让用户挑一个接口救回来', () => {
    const broken = view({ sourceInlineJson: null, sourceEndpointId: null });
    actions().beginEditDynamicVariableDraft(broken);
    assert.equal(draftOf('p1')?.sourceKind, 'endpoint');
  });
});

describe('清理时机', () => {
  it('保存成功与点「取消」都只清自己那个项目的草稿', () => {
    const { beginCreateDynamicVariableDraft, clearDynamicVariableDraft } = actions();
    beginCreateDynamicVariableDraft('p1');
    beginCreateDynamicVariableDraft('p2');

    // 表单的两条出口（保存成功的 onDone 与「取消」）都是这一个动作。
    clearDynamicVariableDraft('p1');

    assert.equal(draftOf('p1'), undefined);
    assert.ok(draftOf('p2'), '另一个项目的草稿不该被一起清掉');
  });

  it('被编辑的那条定义被删掉时，草稿跟着走', async () => {
    const definition = view();
    useAppStore.setState({ dynamicVariables: [definition] });
    actions().beginEditDynamicVariableDraft(definition);

    await actions().deleteDynamicVariable('d-sns');

    assert.equal(draftOf('p1'), undefined, '定义没了，表单已经存不回去');
  });

  it('删的是别的定义时，正在新建的表单留着', async () => {
    const definition = view();
    useAppStore.setState({ dynamicVariables: [definition] });
    actions().beginCreateDynamicVariableDraft('p1');
    actions().patchDynamicVariableDraft('p1', { name: 'typing' });

    await actions().deleteDynamicVariable('d-sns');

    assert.equal(draftOf('p1')?.name, 'typing');
  });

  it('关掉别的标签不清；关掉这个项目的动态变量标签才清', () => {
    const dynamicTab = dynamicTabId('p1');
    useAppStore.setState({
      openTabs: [
        { id: dynamicTab, kind: 'dynamic-variables' },
        { id: 't-other', kind: 'settings' },
      ],
      activeTabId: dynamicTab,
    });
    actions().beginCreateDynamicVariableDraft('p1');

    actions().closeTab('t-other');
    assert.ok(draftOf('p1'), '关别的标签不许带走动态变量草稿');

    actions().closeTab(dynamicTab);
    assert.equal(draftOf('p1'), undefined, '标签没了，草稿就没有「切回来接着填」的归处');
  });

  it('「关闭全部」带走标签条上那份草稿，暂存在别的项目里的不受影响', () => {
    useAppStore.setState({
      openTabs: [{ id: dynamicTabId('p1'), kind: 'dynamic-variables' }],
      activeTabId: dynamicTabId('p1'),
    });
    actions().beginCreateDynamicVariableDraft('p1');
    // p2 的标签已切走（暂存在 projectTabs 里），它还算开着。
    actions().beginCreateDynamicVariableDraft('p2');

    actions().closeAllTabs();

    assert.equal(draftOf('p1'), undefined);
    assert.ok(draftOf('p2'), '暂存的标签还开着，草稿得留着');
  });

  it('切项目只暂存标签，草稿留着（切回来继续填）', () => {
    const dynamicTab = dynamicTabId('p1');
    const p2Tab = dynamicTabId('p2');
    // 两个项目都开着标签：p2 的标签这一轮会被搬进 `projectTabs`，若它根本不在 `openTabs` 里，
    // 这个用例即使把草稿丢掉了也照样绿。
    useAppStore.setState({
      openTabs: [
        { id: dynamicTab, kind: 'dynamic-variables' },
        { id: p2Tab, kind: 'dynamic-variables' },
      ],
      activeTabId: dynamicTab,
    });
    actions().beginCreateDynamicVariableDraft('p1');
    actions().patchDynamicVariableDraft('p1', { name: 'sns' });
    actions().beginCreateDynamicVariableDraft('p2');

    actions().switchProjectTabs('p2');

    assert.equal(draftOf('p1')?.name, 'sns', '标签只是暂存，不是关掉');
    assert.ok(draftOf('p2'), '切过去的那个项目的草稿也不许被这一轮带走');
    assert.deepEqual(
      useAppStore.getState().projectTabs.p1?.tabs.map((t) => t.id),
      [dynamicTab],
      'p1 的标签进了暂存区',
    );
  });

  it('项目被清扫掉时（pruneStaleTabs 退役它的标签）草稿跟着退役', () => {
    useAppStore.setState({
      openTabs: [{ id: dynamicTabId('p1'), kind: 'dynamic-variables' }],
      activeTabId: dynamicTabId('p1'),
    });
    actions().beginCreateDynamicVariableDraft('p1');

    useAppStore.setState({ projects: [project('p2')] });
    actions().pruneStaleTabs();

    assert.equal(draftOf('p1'), undefined);
  });

  it('删项目成功之后草稿立刻走，IPC 失败时留着（回滚不该扔掉表单）', async () => {
    actions().beginCreateDynamicVariableDraft('p1');
    actions().patchDynamicVariableDraft('p1', { name: 'sns' });

    installIpcMock((cmd) => {
      if (cmd === 'projects_delete') throw new Error('boom');
      return [];
    });
    await actions().deleteProject('p1');
    assert.equal(draftOf('p1')?.name, 'sns', '删除失败，项目还在，草稿也得在');

    installIpcMock((cmd) => {
      if (cmd === 'projects_delete') return null;
      return [];
    });
    await actions().deleteProject('p1');
    assert.equal(draftOf('p1'), undefined);
  });
});

describe('草稿让标签变脏：「关闭已保存」不许带走表单', () => {
  it('有草稿时标签进脏集，closeSavedTabs 不关它', () => {
    const dynamicTab = dynamicTabId('p1');
    useAppStore.setState({
      openTabs: [{ id: dynamicTab, kind: 'dynamic-variables' }],
      activeTabId: dynamicTab,
    });
    actions().beginCreateDynamicVariableDraft('p1');
    actions().patchDynamicVariableDraft('p1', { name: 'sns' });

    assert.ok(
      useAppStore.getState().dirtyTabIds.has(dynamicTab),
      '有未保存草稿的动态变量标签必须算脏——否则「关闭已保存」会把表单一起关掉',
    );
    assert.equal(actions().closeSavedTabs(), 0);
    assert.ok(draftOf('p1'), '草稿还在');
  });

  it('草稿清掉之后标签变干净，closeSavedTabs 就能关它', () => {
    const dynamicTab = dynamicTabId('p1');
    useAppStore.setState({
      openTabs: [{ id: dynamicTab, kind: 'dynamic-variables' }],
      activeTabId: dynamicTab,
    });
    actions().beginCreateDynamicVariableDraft('p1');

    actions().clearDynamicVariableDraft('p1');

    assert.equal(useAppStore.getState().dirtyTabIds.has(dynamicTab), false);
    assert.equal(actions().closeSavedTabs(), 1);
    assert.equal(
      useAppStore.getState().openTabs.some((t) => t.id === dynamicTab),
      false,
    );
  });

  it('关掉标签时脏标记一起摘掉', () => {
    const dynamicTab = dynamicTabId('p1');
    useAppStore.setState({
      openTabs: [{ id: dynamicTab, kind: 'dynamic-variables' }],
      activeTabId: dynamicTab,
    });
    actions().beginCreateDynamicVariableDraft('p1');
    assert.ok(useAppStore.getState().dirtyTabIds.has(dynamicTab));

    actions().closeTab(dynamicTab);

    assert.equal(
      useAppStore.getState().dirtyTabIds.has(dynamicTab),
      false,
      '标签没了，脏标记也不该留着',
    );
  });
  it('保存完成只清「提交的那一份」：期间又敲了内容就留着', () => {
    actions().beginEditDynamicVariableDraft(view());
    const submitted = draftOf('p1');
    // 保存请求还在路上时用户又改了一格：草稿换成了新对象。
    actions().patchDynamicVariableDraft('p1', { name: 'sns2' });

    actions().finishDynamicVariableSave('p1', submitted, view({ name: 'sns1' }));

    assert.equal(draftOf('p1')?.name, 'sns2', '新敲的内容不许被这次保存的收尾抹掉');
    assert.equal(
      useAppStore.getState().dirtyTabIds.has(dynamicTabId('p1')),
      true,
      '还有没保存的内容，标签继续算脏',
    );
    // 基准推进到刚落地的那一份：否则「改回原值」会被算成没变而不发出去。
    assert.equal(draftOf('p1')?.openedWith?.name, 'sns1');
  });

  it('保存完成时草稿没变，就清掉并摘脏', () => {
    actions().beginEditDynamicVariableDraft(view());
    const submitted = draftOf('p1');

    actions().finishDynamicVariableSave('p1', submitted, view());

    assert.equal(draftOf('p1'), undefined);
    assert.equal(useAppStore.getState().dirtyTabIds.has(dynamicTabId('p1')), false);
  });

  it('暂存标签的脏标记不被别的项目那次清扫抹掉', () => {
    const p1Tab = dynamicTabId('p1');
    // 标签条上还挂着一个底层实体已经没了的标签：有它，这一轮清扫才会真的动到 `dirtyTabIds`。
    useAppStore.setState({
      openTabs: [
        { id: p1Tab, kind: 'dynamic-variables' },
        { id: 'ep-gone', kind: 'endpoint' },
      ],
      activeTabId: p1Tab,
    });
    actions().beginCreateDynamicVariableDraft('p1');
    assert.equal(useAppStore.getState().dirtyTabIds.has(p1Tab), true);

    // 切到 p2：p1 的标签进了 `projectTabs`，脏标记按 id 记着。
    actions().switchProjectTabs('p2');
    useAppStore.setState({ activeProjectId: 'p2' });
    // p2 里发生一次清扫：它看不到 p1 的标签，但没理由把那条标记一起删了。
    actions().pruneStaleTabs();
    assert.equal(
      useAppStore.getState().dirtyTabIds.has(p1Tab),
      true,
      'pruneStaleTabs 只减被退役的',
    );

    // 先切标签再改活动项目：`switchProjectTabs` 以「当前项目」为准做暂存。
    actions().switchProjectTabs('p1');
    useAppStore.setState({ activeProjectId: 'p1' });

    assert.equal(useAppStore.getState().dirtyTabIds.has(p1Tab), true, '切回来那份半表单仍然是脏的');
    actions().closeSavedTabs();
    assert.equal(
      useAppStore.getState().openTabs.some((t) => t.id === p1Tab),
      true,
      '脏标签不许被「关闭已保存」带走',
    );
    assert.ok(draftOf('p1'));
  });

  it('「关闭全部」只摘真正被关掉的那些脏标记', () => {
    const p1Tab = dynamicTabId('p1');
    const p2Tab = dynamicTabId('p2');
    useAppStore.setState({
      openTabs: [
        { id: p1Tab, kind: 'dynamic-variables' },
        { id: p2Tab, kind: 'dynamic-variables' },
      ],
      activeTabId: p1Tab,
    });
    actions().beginCreateDynamicVariableDraft('p1');
    actions().beginCreateDynamicVariableDraft('p2');
    actions().switchProjectTabs('p2');
    useAppStore.setState({ activeProjectId: 'p2' });
    // p1 的标签与标记都进了暂存区，此刻标签条上只剩 p2 的。
    actions().closeAllTabs();

    const dirty = useAppStore.getState().dirtyTabIds;
    assert.equal(dirty.has(p2Tab), false, '关掉的标签不再脏');
    assert.equal(dirty.has(p1Tab), true, '暂存中的标签仍算开着');

    // 先切标签再改活动项目：`switchProjectTabs` 以「当前项目」为准做暂存。
    actions().switchProjectTabs('p1');
    useAppStore.setState({ activeProjectId: 'p1' });

    actions().closeSavedTabs();
    assert.equal(
      useAppStore.getState().openTabs.some((t) => t.id === p1Tab),
      true,
      '切回来仍是脏的，不该被关掉',
    );
    assert.ok(draftOf('p1'));
  });
  it('新建期间又敲了内容：收尾把草稿转成刚建那条的编辑态，不再算「新建」', () => {
    // 不转的话 `creating` 还挂着，再点保存就是第二次创建，撞项目内的名字唯一约束。
    actions().beginCreateDynamicVariableDraft('p1');
    actions().patchDynamicVariableDraft('p1', {
      name: 'sns',
      sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
    });
    const submitted = draftOf('p1');
    assert.equal(submitted?.creating, true);
    // 请求还在路上，用户又补了一句说明：草稿换成新对象。
    actions().patchDynamicVariableDraft('p1', { description: '后来补的' });

    actions().finishDynamicVariableSave('p1', submitted!, view({ id: 'd-new', name: 'sns' }));

    const draft = draftOf('p1');
    assert.equal(draft?.creating, false, '落库之后就是编辑态');
    assert.equal(draft?.editingId, 'd-new', '再保存要改成这条定义，而不是再建一条');
    assert.equal(draft?.openedWith?.id, 'd-new', '基线换成落库的那一份');
    assert.equal(draft?.description, '后来补的', '期间敲的内容留着');
    assert.equal(
      useAppStore.getState().dirtyTabIds.has(dynamicTabId('p1')),
      true,
      '还有没保存的内容，标签继续算脏',
    );
  });

  it('草稿已被别处清掉时才收尾：什么也不重建', () => {
    actions().beginEditDynamicVariableDraft(view());
    const submitted = draftOf('p1');
    actions().clearDynamicVariableDraft('p1');

    actions().finishDynamicVariableSave('p1', submitted!, view());

    assert.equal(draftOf('p1'), undefined, '取消之后又被一次迟到的保存叫回来，就是幽灵表单');
  });

  it('暂存标签的 endpoint 草稿不被清扫抹掉', () => {
    // 暂存进 `projectTabs` 的标签不在 `openTabs` 里：从 `valid` 重建草稿表就会把它抹掉，
    // 于是它那条脏标记成了「脏而无草稿」。
    const stashed = 'draft:stashed';
    useAppStore.setState({
      openTabs: [{ id: 'ep-gone', kind: 'endpoint' }],
      activeTabId: 'ep-gone',
      endpointDrafts: { [stashed]: { url: '/kept' } } as never,
      dirtyTabIds: new Set([stashed, 'ep-gone']),
      projectTabs: {
        p2: { tabs: [{ id: stashed, kind: 'endpoint-draft' }], activeTabId: stashed },
      },
    });

    actions().pruneStaleTabs();

    const state = useAppStore.getState();
    assert.equal(state.endpointDrafts[stashed]?.url, '/kept', '暂存标签的草稿要留着');
    assert.equal(state.dirtyTabIds.has(stashed), true, '它仍算脏');
    assert.equal(state.endpointDrafts['ep-gone'], undefined, '被清扫掉的那个才真清');
  });

  it('「关闭全部」只带走标签条上那些 endpoint 草稿', () => {
    const live = 'draft:live';
    const stashed = 'draft:stashed';
    useAppStore.setState({
      openTabs: [{ id: live, kind: 'endpoint-draft' }],
      activeTabId: live,
      endpointDrafts: { [live]: { url: '/live' }, [stashed]: { url: '/kept' } } as never,
      draftEndpoints: { [live]: { id: live }, [stashed]: { id: stashed } } as never,
      dirtyTabIds: new Set([live, stashed]),
    });

    actions().closeAllTabs();

    const state = useAppStore.getState();
    assert.equal(state.endpointDrafts[live], undefined, '关掉的那条草稿跟着走');
    assert.equal(state.endpointDrafts[stashed]?.url, '/kept', '暂存标签仍算开着');
    assert.equal(state.draftEndpoints[stashed] !== undefined, true);
    assert.equal(state.draftEndpoints[live], undefined);
  });
  it('保存期间点开另一条定义：收尾不改写那份草稿', () => {
    actions().beginEditDynamicVariableDraft(view({ id: 'd-a', name: 'a' }));
    const submitted = draftOf('p1');
    // 保存还在路上，用户点开了另一条定义的表单。
    actions().beginEditDynamicVariableDraft(view({ id: 'd-b', name: 'b' }));

    actions().finishDynamicVariableSave('p1', submitted!, view({ id: 'd-a', name: 'a' }));

    const draft = draftOf('p1');
    assert.equal(draft?.editingId, 'd-b', '另一条定义的表单不许被上一条的保存收尾劫走');
    assert.equal(draft?.name, 'b');
  });
  it('「关闭已保存」也只带走标签条上那些 endpoint 草稿', () => {
    // 暂存标签的草稿若被抹掉，它那条脏标记还在——于是「脏而无草稿」，再也存不回去。
    const clean = 'ep-clean';
    const stashed = 'draft:stashed';
    useAppStore.setState({
      openTabs: [{ id: clean, kind: 'endpoint' }],
      activeTabId: clean,
      endpointDrafts: { [clean]: { url: '/closed' }, [stashed]: { url: '/kept' } } as never,
      draftEndpoints: { [stashed]: { id: stashed } } as never,
      dirtyTabIds: new Set([stashed]),
      projectTabs: {
        p2: { tabs: [{ id: stashed, kind: 'endpoint-draft' }], activeTabId: stashed },
      },
    });

    assert.equal(actions().closeSavedTabs(), 1, '只有标签条上那条干净的标签会被关');

    const state = useAppStore.getState();
    assert.equal(state.endpointDrafts[clean], undefined, '被关掉的那条的草稿跟着走');
    assert.equal(state.endpointDrafts[stashed]?.url, '/kept', '暂存标签的草稿要留着');
    assert.equal(state.draftEndpoints[stashed] !== undefined, true, '它的文档也留着');
  });
});
