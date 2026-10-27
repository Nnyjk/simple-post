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
 * 取值桶不按环境分桶：定义是全局的，取数环境由定义自己决定，故同一个名字只有一个当前值。
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
  DynamicVariableReference,
  DynamicVariableView,
} from '@/types/domain';

// ---------- harness ----------

/** 一个只带这个切片会用到的字段的最小 store。 */
function makeHarness() {
  const bag: Record<string, any> = {
    dynamicVariables: [] as DynamicVariable[],
    dynamicValues: {} as Record<string, DynamicValueStatus>,
    dynamicErrors: {} as Record<string, DynamicError>,
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
function definition(name: string, id = `d-${name}`): DynamicVariable {
  return {
    id,
    name,
    description: null,
    sourceEndpointId: null,
    sourceEndpointName: null,
    sourceInlineJson: '{"method":"GET","url":"http://127.0.0.1:9/points"}',
    sourceEnvironmentId: null,
    sourceBaseUrlDefinitionId: null,
    extractPath: '$',
    valueMode: 'auto',
    ttlSeconds: 60,
    staleFallback: false,
    enabled: true,
    sortOrder: 0,
    createdAt: 1,
    updatedAt: 1,
  } as DynamicVariable;
}

/** 一条定义 + 它的当前值与引用关系。 */
function view(name: string, referencedBy: DynamicVariableReference[] = []): DynamicVariableView {
  return {
    ...definition(name),
    value: null,
    source: 'missing',
    fetchedAt: null,
    expiresAt: null,
    isOverride: false,
    error: null,
    referencedBy,
  } as DynamicVariableView;
}

/** 一条引用关系。 */
function reference(endpointName: string): DynamicVariableReference {
  return {
    endpointId: `e-${endpointName}`,
    endpointName,
    projectId: 'p1',
    projectName: 'First',
  };
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
      sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus,
    };
    bag.dynamicErrors = { sns: { name: 'sns', kind: 'network', message: '断了' } };

    bag.dropDynamicValue('sns');

    assert.deepEqual(bag.dynamicValues, {});
    assert.deepEqual(bag.dynamicErrors, {});
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
      sns: { name: 'sns', value: '[1]', source: 'auto' } as DynamicValueStatus,
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

    const failed = await bag.resolveDynamicVariables(['sns']);
    assert.equal(failed.errors.length, 1);
    assert.equal(bag.dynamicErrors.sns?.kind, 'timeout');

    phase = 'ok';
    await bag.resolveDynamicVariables(['sns']);
    assert.equal(bag.dynamicErrors.sns, undefined, '取数成功后原因应当被清掉');
  });

  it('解析不带项目与环境参数', async () => {
    // 定义是全局的：解析按名字找定义，取数环境由定义自己决定。
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
    assert.deepEqual(seen[0], { names: ['sns'], force: false });
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

  it('加载时把要不要算引用关系告诉后端', async () => {
    const bag = makeHarness();
    const seen: any[] = [];
    installIpcMock((cmd, args) => {
      if (cmd === 'dynamic_variables_list') {
        seen.push(args);
        return [];
      }
      return null;
    });

    await bag.loadDynamicVariables();
    await bag.loadDynamicVariables({ includeReferences: false });

    assert.deepEqual(seen[0], { includeReferences: true });
    assert.deepEqual(seen[1], { includeReferences: false });
  });

  it('不算引用关系的加载不清空已知的引用关系', async () => {
    const bag = makeHarness();
    // 面板算过一次引用关系：这条列表里带着它。
    bag.dynamicVariables = [view('sns', [reference('消费接口')])];
    installIpcMock((cmd) => {
      // 后端不算引用关系时带回来的 `referencedBy` 是空的。
      if (cmd === 'dynamic_variables_list') return [view('sns')];
      return null;
    });

    await bag.loadDynamicVariables({ includeReferences: false });

    assert.equal(bag.dynamicVariables.length, 1);
    assert.equal(bag.dynamicVariables[0].referencedBy.length, 1);
    assert.equal(bag.dynamicVariables[0].referencedBy[0].endpointName, '消费接口');
  });

  it('算引用关系的加载用后端给的新值', async () => {
    const bag = makeHarness();
    bag.dynamicVariables = [view('sns', [reference('旧接口')])];
    installIpcMock((cmd) => {
      if (cmd === 'dynamic_variables_list') return [view('sns', [reference('新接口')])];
      return null;
    });

    await bag.loadDynamicVariables({ includeReferences: true });

    assert.equal(bag.dynamicVariables[0].referencedBy.length, 1);
    assert.equal(bag.dynamicVariables[0].referencedBy[0].endpointName, '新接口');
  });
});
