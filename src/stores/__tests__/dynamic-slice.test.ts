/**
 * `src/stores/dynamic-slice.ts` 的切片级测试。
 *
 * 覆盖的职责：
 *   - `dropDynamicValue` 把一个变量名从取值桶里剔除，并清掉它的失败原因。
 *     留着旧值就等于停用或删除之后仍然在替换。
 *   - `deleteDynamicVariable` 与 `updateDynamicVariable` 在改动之后剔除旧名字的值。
 *   - `resolveDynamicVariables` 按变量名记下取数失败的原因，成功时清掉。
 *   - `updateDynamicVariable` 失败时返回 false（表单据此不关闭，用户输入不丢）。
 *
 * 取值桶**按项目 × 取数环境分桶**（`dynamicValues[dynamicValueScope(projectId, env)][name]`）：
 * 定义挂在项目里，不同项目可以有同名变量而取值不同；同一份定义换个环境取到的也是另一份
 * 值，故一格装不下它们。
 *
 * 运行：
 *   npx tsx --test src/stores/__tests__/dynamic-slice.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, installIpcMock, clearIpcMock } from './_browser-shim';

import {
  creatingDraftKey,
  createDynamicSlice,
  dynamicListMatchesEnvironment,
  dynamicValueScope,
} from '../dynamic-slice';
import type {
  DynamicError,
  DynamicValueStatus,
  DynamicVariable,
  DynamicVariableView,
} from '@/types/domain';

// ---------- harness ----------

/** 一个只带这个切片会用到的字段的最小 store。 */
function makeHarness() {
  const bag: Record<string, any> = {
    dynamicVariables: [] as DynamicVariable[],
    dynamicValues: {} as Record<string, Record<string, DynamicValueStatus>>,
    dynamicErrors: {} as Record<string, Record<string, DynamicError>>,
    dynamicInFlight: 0,
    savingDynamicVariableIds: new Set<string>(),
    dynamicDrafts: {} as Record<string, unknown>,
    dynamicVariablesScope: null as string | null,
    dirtyTabIds: new Set<string>(),
    dynamicError: null as string | null,
    activeProjectId: 'p1',
    activeEnvironmentId: 'env1',
  };
  const set = (partial: any) => {
    const updater = typeof partial === 'function' ? partial(bag) : partial;
    if (updater) Object.assign(bag, updater);
  };
  const get = () => bag;

  const slice = createDynamicSlice(set as any, get as any, {} as any);
  // 重新绑到 `bag` 上，切片方法之间互相调用（`loadDynamicVariables` 等）才解析得到。
  Object.assign(bag, slice);
  return bag;
}

/** 一条定义。 */
function definition(name: string, id = `d-${name}`, projectId = 'p1'): DynamicVariable {
  return {
    id,
    projectId,
    name,
    description: null,
    sourceEndpointId: null,
    sourceEndpointName: null,
    sourceInlineJson: '{"method":"GET","url":"http://127.0.0.1:9/points"}',
    sourceEnvironmentId: null,
    extractPath: '$',
    transformScript: null,
    valueMode: 'auto',
    ttlSeconds: 60,
    staleFallback: false,
    enabled: true,
    sortOrder: 0,
    createdAt: 1,
    updatedAt: 1,
  } as DynamicVariable;
}

/** 一条定义 + 它的当前值。 */
function view(name: string, projectId = 'p1'): DynamicVariableView {
  return {
    ...definition(name, `d-${name}`, projectId),
    value: null,
    source: 'missing',
    fetchedAt: null,
    expiresAt: null,
    isOverride: false,
    error: null,
  } as DynamicVariableView;
}

/** 一次解析的结果。 */
function resolution(names: string[], values: Record<string, string>, errors: DynamicError[] = []) {
  return {
    values,
    statuses: names.map((name): DynamicValueStatus => ({
      name,
      value: values[name] ?? null,
      source: values[name] !== undefined ? 'auto' : 'missing',
      fetchedAt: 1,
      expiresAt: 2,
      isOverride: false,
      error: null,
    })),
    errors,
  };
}

// ---------- 用例 ----------

/** 当前 harness 里那个项目的取值格键（`env1` 是 harness 的 `activeEnvironmentId`）。 */
const scope1 = dynamicValueScope('p1', 'env1');
const scope2 = dynamicValueScope('p2', 'env1');

describe('dynamic slice', () => {
  let shim: ReturnType<typeof installBrowserShim>;

  beforeEach(() => {
    shim = installBrowserShim();
  });

  afterEach(() => {
    clearIpcMock();
    shim.teardown();
  });

  it('dropDynamicValue 清掉那个名字与它的失败原因', () => {
    const bag = makeHarness();
    bag.dynamicValues = {
      [scope1]: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
    };
    bag.dynamicErrors = {
      [scope1]: { sns: { name: 'sns', kind: 'network', message: '断了' } },
    };

    bag.dropDynamicValue('p1', 'sns');

    assert.deepEqual(bag.dynamicValues, {});
    assert.deepEqual(bag.dynamicErrors, {});
  });

  it('dropDynamicValue 不动别的项目里的同名变量', () => {
    // 同名变量在两个项目里是两条不同的定义，故一个项目的值不能被另一个项目顺手删掉。
    const bag = makeHarness();
    bag.dynamicValues = {
      [scope1]: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
      [scope2]: { sns: { name: 'sns', value: '[2]', source: 'auto' } as DynamicValueStatus },
    };

    bag.dropDynamicValue('p1', 'sns');

    assert.equal(bag.dynamicValues[scope1], undefined, 'p1 那一格应当被清空');
    assert.equal(bag.dynamicValues[scope2]?.sns?.value, '[2]', 'p2 那一格不该被动');
  });

  it('删掉一条定义之后它的旧值不再留在桶里', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [definition('sns')];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_delete') return null;
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    await bag.deleteDynamicVariable('d-sns');

    assert.deepEqual(bag.dynamicValues, {}, '旧值应当被剔除');
  });

  it('改名之后旧名字下的值被剔除', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [definition('sns')];
    bag.dynamicValues = {
      [scope1]: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
    };
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_update') return definition('points');
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    await bag.updateDynamicVariable('d-sns', { name: 'points' });

    assert.deepEqual(bag.dynamicValues, {}, '旧名字的值应当被剔除');
  });

  it('取数失败的原因按变量名记下来，取数成功后清掉', async () => {
    const bag = makeHarness();
    const failure: DynamicError = { name: 'sns', kind: 'timeout', message: '超时了' };
    let phase: 'fail' | 'ok' = 'fail';
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_resolve') {
        return phase === 'fail'
          ? resolution([], {}, [failure])
          : resolution(['sns'], { sns: '[1]' });
      }
      return null;
    });

    const failed = await bag.resolveDynamicVariables(['sns'], 'p1');
    assert.equal(failed.errors.length, 1);
    assert.equal(bag.dynamicErrors[scope1]?.sns?.kind, 'timeout');

    phase = 'ok';
    await bag.resolveDynamicVariables(['sns'], 'p1');
    assert.equal(bag.dynamicErrors[scope1]?.sns, undefined, '取数成功后原因应当被清掉');
  });

  it('同名变量在两个项目里各有一份，取值桶不互相覆盖', async () => {
    // 这正是「挂到项目」要解决的那件事：两个项目各有一个 `sns`，取值不同，
    // 切到另一个项目时不能把上一个项目的值带过去。
    const bag = makeHarness();
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_resolve') {
        const projectId = (args as { projectId?: string | null }).projectId ?? '';
        return resolution(['sns'], { sns: `[${projectId}]` });
      }
      return null;
    });

    await bag.resolveDynamicVariables(['sns'], 'p1');
    await bag.resolveDynamicVariables(['sns'], 'p2');

    assert.equal(bag.dynamicValues[scope1]?.sns?.value, '[p1]');
    assert.equal(bag.dynamicValues[scope2]?.sns?.value, '[p2]');
    assert.notEqual(
      bag.dynamicValues[scope1],
      bag.dynamicValues[scope2],
      '两个项目的取值桶必须是两份，而不是同一个对象',
    );
  });

  it('解析带上项目：请求侧只认消费方项目', async () => {
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_resolve') {
        seen.push(args);
        return resolution(['sns'], { sns: '[1]' });
      }
      return null;
    });

    await bag.resolveDynamicVariables(['sns'], 'p1');

    assert.equal(seen.length, 1);
    // 取数环境随命令一起送上去：继承取数环境的定义按调用方选中的环境取值。
    assert.deepEqual(seen[0], {
      names: ['sns'],
      projectId: 'p1',
      environmentId: 'env1',
      force: false,
    });
  });

  it('解析不带项目就是跨项目取值', async () => {
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_resolve') {
        seen.push(args);
        return resolution(['sns'], { sns: '[1]' });
      }
      return null;
    });

    await bag.resolveDynamicVariables(['sns']);

    assert.equal(seen.length, 1);
    assert.deepEqual(seen[0], {
      names: ['sns'],
      projectId: null,
      environmentId: 'env1',
      force: false,
    });
  });

  it('切了环境，解析带上的是新环境', async () => {
    // 「继承当前环境」的定义按调用方此刻选中的那个环境取数：切环境后同一份命令必须换一段
    // 环境 id 送上去，否则用户看到的还是上一个环境的值。
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_resolve') {
        seen.push(args);
        return resolution(['sns'], { sns: '[1]' });
      }
      return null;
    });

    await bag.resolveDynamicVariables(['sns'], 'p1');
    bag.activeEnvironmentId = 'env2';
    await bag.resolveDynamicVariables(['sns'], 'p1');

    assert.deepEqual(
      seen.map((a) => a.environmentId),
      ['env1', 'env2'],
    );
  });

  it('刷新与写覆盖值也带上调用方的环境', async () => {
    // 缓存键是 (定义 × 生效环境)，故这两条命令落在哪一格都取决于这段环境 id。
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      seen.push([cmd, args]);
      if (cmd === 'dynamic_variables_list') return [view('sns')];
      return null;
    });

    await bag.refreshDynamicVariable('d-sns');
    await bag.setDynamicOverride('d-sns', 'v');

    const refresh = seen.find(([cmd]) => cmd === 'dynamic_variables_refresh');
    const override = seen.find(([cmd]) => cmd === 'dynamic_variables_set_override');
    assert.equal(refresh?.[1].environmentId, 'env1');
    assert.equal(override?.[1].environmentId, 'env1');
  });

  it('保存失败时 updateDynamicVariable 返回 null', async () => {
    const bag = makeHarness();
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_update') throw new Error('名字重复');
      return null;
    });

    const saved = await bag.updateDynamicVariable('d-sns', { name: 'sns' });
    assert.equal(saved, null);
    assert.match(bag.dynamicError ?? '', /名字重复/);
  });

  it('加载列表带上项目', async () => {
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_list') {
        seen.push(args);
        return [view('sns')];
      }
      return null;
    });

    await bag.loadDynamicVariables('p1');

    assert.equal(seen.length, 1);
    assert.deepEqual(seen[0], { projectId: 'p1', environmentId: 'env1' });
    assert.equal(bag.dynamicVariables.length, 1);
    assert.equal(bag.dynamicVariables[0].name, 'sns');
  });

  it('加载列表不带项目就是跨项目那一路', async () => {
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_list') {
        seen.push(args);
        return [view('sns')];
      }
      return null;
    });

    await bag.loadDynamicVariables();

    assert.deepEqual(seen[0], { projectId: null, environmentId: 'env1' });
  });

  it('新建定义带上归属项目', async () => {
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_create') {
        seen.push(args);
        return definition('sns');
      }
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const created = await bag.createDynamicVariable({
      projectId: 'p2',
      name: 'sns',
      sourceInlineJson: '{"method":"GET","url":"http://127.0.0.1:9/points"}',
    });

    assert.equal(created?.id, 'd-sns');
    assert.equal(seen[0].projectId, 'p2');
  });
  it('列表回来得早的那一次不会把晚发出的那一次挤掉', async () => {
    // 切项目 / 切环境时上一次请求还在路上：它带回来的是**上一个环境**下的当前值，
    // 照单全收就是把别的环境的值显示成这个环境的。
    const bag = makeHarness();
    const pending: Array<(views: DynamicVariableView[]) => void> = [];
    installIpcMock((cmd) => {
      if (cmd !== 'dynamic_variables_list') return null;
      return new Promise<DynamicVariableView[]>((resolve) => {
        pending.push(resolve);
      });
    });

    const slow = bag.loadDynamicVariables('p1');
    const fast = bag.loadDynamicVariables('p2');
    assert.equal(pending.length, 2);

    // 后发的先回来：它才是当前这份。
    pending[1]([view('points', 'p2')]);
    await fast;
    // 先发的后回来：丢掉，不许覆盖上屏。
    pending[0]([view('sns', 'p1')]);
    await slow;

    assert.deepEqual(
      bag.dynamicVariables.map((v: DynamicVariableView) => v.name),
      ['points'],
      '上屏的必须是最后发起的那一次的结果',
    );
  });

  it('同一格连点两次取数：先发的那次结果丢掉，后发的算数', async () => {
    const bag = makeHarness();
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd !== 'dynamic_variables_resolve') return null;
      return new Promise((resolve) => {
        pending.push(resolve);
      });
    });

    const first = bag.resolveDynamicVariables(['sns'], 'p1');
    const second = bag.resolveDynamicVariables(['sns'], 'p1');
    assert.equal(pending.length, 2);

    pending[1](resolution(['sns'], { sns: '[new]' }));
    await second;
    pending[0](resolution(['sns'], { sns: '[old]' }));
    await first;

    assert.equal(bag.dynamicValues[scope1]?.sns?.value, '[new]', '晚回来的旧结果不许把新值盖回去');
  });

  it('在不同环境里取到的值各占一格，切回来各读各的', async () => {
    const bag = makeHarness();
    installIpcMock((cmd, args) => {
      if (cmd !== 'dynamic_variables_resolve') return null;
      const environmentId = (args as { environmentId?: string | null }).environmentId ?? '';
      return resolution(['sns'], { sns: `[${environmentId}]` });
    });

    await bag.resolveDynamicVariables(['sns'], 'p1');
    bag.activeEnvironmentId = 'env2';
    await bag.resolveDynamicVariables(['sns'], 'p1');

    const env2Scope = dynamicValueScope('p1', 'env2');
    assert.equal(bag.dynamicValues[scope1]?.sns?.value, '[env1]', 'A 环境那一格留着 A 的值');
    assert.equal(bag.dynamicValues[env2Scope]?.sns?.value, '[env2]', 'B 环境那一格是 B 的值');
    assert.notEqual(scope1, env2Scope, '两格的键必须不同');
  });

  it('同一个变量在不同环境下取数失败，原因也只落各自那一格', async () => {
    const bag = makeHarness();
    let phase: 'fail' | 'ok' = 'fail';
    installIpcMock((cmd) => {
      if (cmd !== 'dynamic_variables_resolve') return null;
      return phase === 'fail'
        ? resolution([], {}, [{ name: 'sns', kind: 'timeout', message: '超时了' }])
        : resolution(['sns'], { sns: '[1]' });
    });

    await bag.resolveDynamicVariables(['sns'], 'p1');
    bag.activeEnvironmentId = 'env2';
    phase = 'ok';
    await bag.resolveDynamicVariables(['sns'], 'p1');

    assert.equal(bag.dynamicErrors[scope1]?.sns?.kind, 'timeout', 'A 环境那次失败的原因留着');
    assert.equal(
      bag.dynamicErrors[dynamicValueScope('p1', 'env2')]?.sns,
      undefined,
      'B 环境取数成功，不许顺手清掉 A 的原因',
    );
  });

  it('改名或删除时清掉这个项目每一格里的旧名字', () => {
    const bag = makeHarness();
    const env2Scope = dynamicValueScope('p1', 'env2');
    bag.dynamicValues = {
      [scope1]: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
      [env2Scope]: { sns: { name: 'sns', value: '[2]', source: 'auto' } as DynamicValueStatus },
    };

    bag.dropDynamicValue('p1', 'sns');

    assert.deepEqual(bag.dynamicValues, {}, '每一格都要清：清的时候不一定站在当初取数的那一格上');
  });
  it('被顶掉的那次解析不写 store，但把自己那份结果照给调用方', async () => {
    // 发送路径要拿这次解析的值去替换占位符：丢掉返回值等于让用户发出一条带 `{{sns}}` 的请求。
    const bag = makeHarness();
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd !== 'dynamic_variables_resolve') return null;
      return new Promise((resolve) => {
        pending.push(resolve);
      });
    });

    const first = bag.resolveDynamicVariables(['sns'], 'p1');
    const second = bag.resolveDynamicVariables(['sns'], 'p1');
    pending[1](resolution(['sns'], { sns: '[new]' }));
    await second;
    pending[0](resolution(['sns'], { sns: '[old]' }));
    const stale = await first;

    assert.equal(bag.dynamicValues[scope1]?.sns?.value, '[new]', 'store 只认最后一次');
    assert.equal(stale.values.sns, '[old]', '调用方拿到的是它自己那一次的结果');
  });

  it('同格另一个变量点刷新，不挡成批解析里其它变量的结果', async () => {
    // 世代号按「格 × 变量名」记：按整格记的话，一条刷新就能让整批结果作废，
    // 那几个变量在变量池里就缺了。
    const bag = makeHarness();
    bag.dynamicVariables = [view('a'), view('b'), view('c')];
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_resolve') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      if (cmd === 'dynamic_variables_refresh') {
        return { name: 'c', value: '[c-fresh]', source: 'auto', isOverride: false, error: null };
      }
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const batch = bag.resolveDynamicVariables(['a', 'b'], 'p1');
    await bag.refreshDynamicVariable('d-c');
    assert.equal(pending.length, 1);
    pending[0](resolution(['a', 'b'], { a: '[a]', b: '[b]' }));
    await batch;

    assert.equal(bag.dynamicValues[scope1]?.a?.value, '[a]', 'a 的结果要落地');
    assert.equal(bag.dynamicValues[scope1]?.b?.value, '[b]', 'b 的结果也要落地');
  });

  it('解析在飞时改了定义：回来的旧规则结果不落进桶里', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [view('sns')];
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_resolve') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      if (cmd === 'dynamic_variables_update') return definition('sns');
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const inFlight = bag.resolveDynamicVariables(['sns'], 'p1');
    await bag.updateDynamicVariable('d-sns', { extractPath: '$.other' });
    pending[0](resolution(['sns'], { sns: '[old-rule]' }));
    const stale = await inFlight;

    assert.equal(bag.dynamicValues[scope1]?.sns, undefined, '按旧抽取规则取到的值不许留在桶里');
    assert.equal(stale.values.sns, '[old-rule]', '调用方仍拿到它自己那一次的结果');
  });
  it('刷新把取到的那份值写进对应格子的桶里，且只清这条定义自己的失败原因', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [view('c')];
    // 先种两条失败原因：这条定义自己那条要被清掉，别的变量那条不许动。桶本来是空的，
    // 「刷新顺手清空整桶」也会让断言通过。
    bag.dynamicErrors = {
      [scope1]: {
        c: { name: 'c', kind: 'network', message: '断了' } as DynamicError,
        other: { name: 'other', kind: 'timeout', message: '超时' } as DynamicError,
      },
    };
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_refresh') {
        return { name: 'c', value: '[c-fresh]', source: 'auto', isOverride: false, error: null };
      }
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    await bag.refreshDynamicVariable('d-c');

    assert.equal(bag.dynamicValues[scope1]?.c?.value, '[c-fresh]', '面板要立刻显示这次重取的值');
    assert.equal(bag.dynamicErrors[scope1]?.c, undefined, '重取成功要清掉上一次的失败原因');
    assert.equal(bag.dynamicErrors[scope1]?.other?.message, '超时', '别的变量的原因不归这次刷新管');
  });

  it('同一条定义在保存中时再提交被挡回，且标志随请求结束清掉', async () => {
    // web 传输下两个 PATCH 可以乱序落地：旧的那个后到就会盖掉新编辑。挡住的是这一条定义。
    const bag = makeHarness();
    bag.dynamicVariables = [view('sns')];
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_update') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const first = bag.updateDynamicVariable('d-sns', { name: 'sns' });
    assert.equal(bag.savingDynamicVariableIds.has('d-sns'), true, '在飞期间表单要禁用保存');
    const second = bag.updateDynamicVariable('d-sns', { name: 'sns2' });
    // 等一轮微任务：被挡住的那次立刻结算，不会再多发一个 PATCH。
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(pending.length, 1, '同一条定义的第二次提交不该再发 PATCH');
    assert.equal(await second, null, '同一条定义的第二次提交直接挡回');

    pending[0](definition('sns'));
    await first;
    assert.equal(bag.savingDynamicVariableIds.has('d-sns'), false, '结束了就要放开');
  });

  it('保存失败也放开那条定义的保存中标志', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [view('sns')];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_update') throw new Error('网络断了');
      return null;
    });

    assert.equal(await bag.updateDynamicVariable('d-sns', { name: 'sns' }), null);
    assert.equal(bag.savingDynamicVariableIds.has('d-sns'), false, '失败也要放开，否则再也存不了');
  });

  it('作废只按项目作废：别的项目里同名变量照常落地', async () => {
    const bag = makeHarness();
    const p2View = { ...view('sns', 'p2'), id: 'd-p2-sns' } as DynamicVariableView;
    bag.dynamicVariables = [view('sns'), p2View];
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_resolve') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      if (cmd === 'dynamic_variables_update') return definition('sns');
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const otherProject = bag.resolveDynamicVariables(['sns'], 'p2');
    await bag.updateDynamicVariable('d-sns', { extractPath: '$.other' });
    pending[0](resolution(['sns'], { sns: '[p2]' }));
    await otherProject;

    assert.equal(
      bag.dynamicValues[dynamicValueScope('p2', 'env1')]?.sns?.value,
      '[p2]',
      'p2 的在飞结果与 p1 的保存无关',
    );
  });

  it('删除之后迟到的解析不把值加回桶里', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [view('sns')];
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_resolve') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      if (cmd === 'dynamic_variables_delete') return null;
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const inFlight = bag.resolveDynamicVariables(['sns'], 'p1');
    await bag.deleteDynamicVariable('d-sns');
    pending[0](resolution(['sns'], { sns: '[gone]' }));
    await inFlight;

    assert.equal(bag.dynamicValues[scope1]?.sns, undefined, '定义没了，迟到的值不许回来');
  });

  it('列表带上它是在哪个环境下拉到的，被顶掉的那一次不写环境戳', async () => {
    const bag = makeHarness();
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_list') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      return null;
    });

    const first = bag.loadDynamicVariables('p1');
    assert.equal(bag.dynamicVariablesScope, null, '还没载到就先别声称对得上');

    // 切了环境：面板又发一次。回来的顺序可能反过来。
    bag.activeEnvironmentId = 'env2';
    const second = bag.loadDynamicVariables('p1');

    pending[1]([]);
    await second;
    assert.equal(bag.dynamicVariablesScope, dynamicValueScope('p1', 'env2'));

    // 旧环境那一次晚回来：它带的是 env1 的值，戳不许改回去。
    pending[0]([]);
    await first;
    assert.equal(
      bag.dynamicVariablesScope,
      dynamicValueScope('p1', 'env2'),
      '被顶掉的列表不能把环境戳改回旧环境',
    );
    assert.equal(
      dynamicListMatchesEnvironment(bag.dynamicVariablesScope, 'p1', 'env1'),
      false,
      '戳不一致时面板不该让用户照着一份旧环境的值写覆盖值',
    );
    assert.equal(dynamicListMatchesEnvironment(bag.dynamicVariablesScope, 'p1', 'env2'), true);
  });

  it('新建在飞时标志按项目记，第二次创建被挡回，结束就放开', async () => {
    // 新建时还没有定义 id；这个标志要能跨表单卸载存活，否则切走标签再回来能再点一次保存。
    const bag = makeHarness();
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_create') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      if (cmd === 'dynamic_variables_list') return [];
      return null;
    });

    const first = bag.createDynamicVariable({ projectId: 'p1', name: 'sns' });
    assert.equal(bag.savingDynamicVariableIds.has(creatingDraftKey('p1')), true);
    const second = bag.createDynamicVariable({ projectId: 'p1', name: 'sns' });
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(pending.length, 1, '同一个项目的第二次新建不该再发 POST');
    assert.equal(await second, null, '第二次新建直接挡回');

    pending[0](definition('sns'));
    await first;
    assert.equal(bag.savingDynamicVariableIds.has(creatingDraftKey('p1')), false);
  });

  it('两个值请求并发时，先回来的那个不把「正在取数」摘掉', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [view('a'), view('b')];
    const pending: Array<(value: unknown) => void> = [];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_resolve') {
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      }
      return null;
    });

    const batch = bag.resolveDynamicVariables(['a'], 'p1');
    const single = bag.resolveDynamicVariables(['b'], 'p1');
    assert.equal(bag.dynamicInFlight, 2, '两条都在路上');

    pending[0](resolution(['a'], { a: '[a]' }));
    await batch;
    assert.equal(bag.dynamicInFlight, 1, '整批还没回来，按钮不许恢复成可点');

    pending[1](resolution(['b'], { b: '[b]' }));
    await single;
    assert.equal(bag.dynamicInFlight, 0, '全部结束才归零');
  });

  it('被顶掉的列表请求失败时不写错误原因', async () => {
    const bag = makeHarness();
    const pending: Array<{ resolve: (v: unknown) => void; reject: (e: unknown) => void }> = [];
    installIpcMock((cmd) => {
      if (cmd !== 'dynamic_variables_list') return null;
      return new Promise((resolve, reject) => {
        pending.push({ resolve, reject });
      });
    });

    const first = bag.loadDynamicVariables('p1');
    const second = bag.loadDynamicVariables('p2');
    pending[1].resolve([view('points', 'p2')]);
    await second;
    pending[0].reject(new Error('p1 那次挂了'));
    await first;

    assert.equal(bag.dynamicError, null, '旧项目那次失败不该盖到新那一屏上');
    assert.equal(bag.dynamicVariables[0]?.projectId, 'p2', '留下的是后发那次的结果');
  });
});
