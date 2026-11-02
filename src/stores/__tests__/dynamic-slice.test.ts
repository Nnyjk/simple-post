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
 * 取值桶**按项目分桶**（`dynamicValues[projectId][name]`）：定义挂在项目里，不同项目
 * 可以有同名变量而取值不同，故一份桶装不下它们两个。
 *
 * 运行：
 *   npx tsx --test src/stores/__tests__/dynamic-slice.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, installIpcMock, clearIpcMock } from './_browser-shim';

import { createDynamicSlice } from '../dynamic-slice';
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
      p1: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
    };
    bag.dynamicErrors = { p1: { sns: { name: 'sns', kind: 'network', message: '断了' } } };

    bag.dropDynamicValue('p1', 'sns');

    assert.deepEqual(bag.dynamicValues, {});
    assert.deepEqual(bag.dynamicErrors, {});
  });

  it('dropDynamicValue 不动别的项目里的同名变量', () => {
    // 同名变量在两个项目里是两条不同的定义，故一个项目的值不能被另一个项目顺手删掉。
    const bag = makeHarness();
    bag.dynamicValues = {
      p1: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
      p2: { sns: { name: 'sns', value: '[2]', source: 'auto' } as DynamicValueStatus },
    };

    bag.dropDynamicValue('p1', 'sns');

    assert.equal(bag.dynamicValues.p1, undefined, 'p1 那一格应当被清空');
    assert.equal(bag.dynamicValues.p2?.sns?.value, '[2]', 'p2 那一格不该被动');
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
      p1: { sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus },
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
    assert.equal(bag.dynamicErrors.p1?.sns?.kind, 'timeout');

    phase = 'ok';
    await bag.resolveDynamicVariables(['sns'], 'p1');
    assert.equal(bag.dynamicErrors.p1?.sns, undefined, '取数成功后原因应当被清掉');
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

    assert.equal(bag.dynamicValues.p1?.sns?.value, '[p1]');
    assert.equal(bag.dynamicValues.p2?.sns?.value, '[p2]');
    assert.notEqual(
      bag.dynamicValues.p1,
      bag.dynamicValues.p2,
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
    assert.deepEqual(seen[0], { names: ['sns'], projectId: 'p1', force: false });
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
    assert.deepEqual(seen[0], { names: ['sns'], projectId: null, force: false });
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
    assert.deepEqual(seen[0], { projectId: 'p1' });
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

    assert.deepEqual(seen[0], { projectId: null });
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
});
