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

import { createDynamicSlice, dynamicValueScope } from '../dynamic-slice';
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
    dynamicResolving: false,
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

  it('保存失败时 updateDynamicVariable 返回 false', async () => {
    const bag = makeHarness();
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_update') throw new Error('名字重复');
      return null;
    });

    const ok = await bag.updateDynamicVariable('d-sns', { name: 'sns' });
    assert.equal(ok, false);
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

    assert.equal(created, 'd-sns');
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
});
