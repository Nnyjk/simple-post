/**
 * `crud-slice` 的环境值删除契约。
 *
 * 覆盖变量与模块地址格子的 null 语义：null = 该字段保持原值，空串 = 清空该字段，
 * 两个字段都清空时整格消失；删除立即移除本地键并发 `null`（空串是真实值，不被当成
 * 删除），失败时整体回滚，以及重命名只发一次 `environments_rename_var` 而非拆成
 * 删除 + 写入。乐观回滚见 `crud-slice-endpoint-detail.test.ts`。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import {
  makeCrudHarness,
  makeDetail,
  makeEnvironment,
  type CapturedInvoke,
} from './crud-slice-harness';
describe('crud-slice: 环境值的 null 删除契约', () => {
  it('变量删除立即移除本地键，发送 null，并在成功路径保留空字符串值', async () => {
    const original = makeEnvironment({
      variables: { token: 'secret', empty: '' },
    });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });

    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      await pending;
      return null;
    });

    const action = harness.slice.setEnvironmentVar('env-1', 'token', null);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      { empty: '' },
      'null 应在 IPC 完成前从乐观状态移除变量',
    );

    release();
    await action;
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_var',
        args: { envId: 'env-1', key: 'token', value: null },
      },
    ]);

    await harness.slice.setEnvironmentVar('env-1', '', '');
    assert.deepEqual(
      harness.bag.environments[0].variables,
      { empty: '', '': '' },
      '空字符串是真实变量值，不应被当作删除',
    );
  });

  it('变量删除失败时恢复原变量和值', async () => {
    const original = makeEnvironment({ variables: { token: 'secret', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => {
      throw new Error('variable delete failed');
    });

    await harness.slice.setEnvironmentVar('env-1', 'token', null);

    assert.deepEqual(harness.bag.environments[0].variables, original.variables);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('模块地址的两个字段各自独立：null 保持原值，空串清空该字段', async () => {
    const original = makeEnvironment({
      moduleBaseUrls: {
        'mod-1': { url: 'https://api.example.test', proxy: 'https://gw.example.test' },
        'mod-2': { url: 'https://api2.example.test' },
      },
    });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    // 只改地址，代理保持原值（proxy 送 null）。
    await harness.slice.setEnvironmentModuleBaseUrl(
      'env-1',
      'mod-1',
      'https://api3.example.test',
      null,
    );
    assert.deepEqual(harness.bag.environments[0].moduleBaseUrls['mod-1'], {
      url: 'https://api3.example.test',
      proxy: 'https://gw.example.test',
    });

    // 只清代理，地址保持原值（url 送 null）。
    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'mod-1', null, '');
    assert.deepEqual(harness.bag.environments[0].moduleBaseUrls['mod-1'], {
      url: 'https://api3.example.test',
      proxy: null,
    });

    // 两个字段都空了，这一格整个消失；别的模块的格子原样保留。
    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'mod-1', '', '');
    assert.equal(
      Object.hasOwn(harness.bag.environments[0].moduleBaseUrls, 'mod-1'),
      false,
      '两个字段都空了，这一格应从乐观状态移除',
    );
    assert.deepEqual(harness.bag.environments[0].moduleBaseUrls['mod-2'], {
      url: 'https://api2.example.test',
    });

    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_module_baseurl',
        args: {
          envId: 'env-1',
          moduleId: 'mod-1',
          url: 'https://api3.example.test',
          proxy: null,
        },
      },
      {
        cmd: 'environments_set_module_baseurl',
        args: { envId: 'env-1', moduleId: 'mod-1', url: null, proxy: '' },
      },
      {
        cmd: 'environments_set_module_baseurl',
        args: { envId: 'env-1', moduleId: 'mod-1', url: '', proxy: '' },
      },
    ]);
  });

  it('模块地址写入失败时恢复原单元格和值', async () => {
    const original = makeEnvironment({
      moduleBaseUrls: { 'mod-1': { url: 'https://api.example.test' } },
    });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => {
      throw new Error('base URL write failed');
    });

    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'mod-1', '', null);

    assert.deepEqual(harness.bag.environments[0].moduleBaseUrls, original.moduleBaseUrls);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('重命名只发一次 environments_rename_var，不再拆成删除+写入两次调用', async () => {
    // 从两次 `environments_set_var`（先 null 再赋值）改成一次调用，是因为前者是两个
    // 事务、中间有一个变量不存在的窗口。现在改名是一次原子的后端调用。
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    const echoed = makeEnvironment({ variables: { renamed: 'value', keep: 'yes' } });
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return echoed;
    });

    const action = harness.slice.renameEnvironmentVar('env-1', 'old', 'renamed');

    // 乐观：交换立刻可见，中间没有变量缺失的状态。
    assert.deepEqual(harness.bag.environments[0].variables, {
      renamed: 'value',
      keep: 'yes',
    });

    await action;

    assert.deepEqual(calls, [
      {
        cmd: 'environments_rename_var',
        args: { envId: 'env-1', fromKey: 'old', toKey: 'renamed' },
      },
    ]);
    assert.deepEqual(harness.bag.environments[0].variables, {
      renamed: 'value',
      keep: 'yes',
    });
  });

  it('重命名失败时恢复完整快照 — 后端从未进入过删除状态', async () => {
    const original = makeEnvironment({ variables: { old: 'value', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      throw new Error('rename failed');
    });

    await harness.slice.renameEnvironmentVar('env-1', 'old', 'renamed');

    assert.deepEqual(calls, [
      {
        cmd: 'environments_rename_var',
        args: { envId: 'env-1', fromKey: 'old', toKey: 'renamed' },
      },
    ]);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      original.variables,
      '单次事务失败时变量必须完好，不存在“已删除未重建”的中间态',
    );
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('重命名的 key 不存在时不发 IPC', async () => {
    const original = makeEnvironment({ variables: { keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.renameEnvironmentVar('env-1', 'ghost', 'renamed');

    assert.deepEqual(calls, [], '源 key 不存在时不应打扰后端');
    assert.deepEqual(harness.bag.environments[0].variables, { keep: 'yes' });
  });

  it('停用一个变量：乐观加入集合并发出一次命令', async () => {
    const original = makeEnvironment({ variables: { token: 'abc', keep: 'yes' } });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.setEnvironmentVarDisabled('env-1', 'token', true);

    assert.deepEqual(harness.bag.environments[0].disabledVariables, ['token']);
    assert.deepEqual(
      harness.bag.environments[0].variables,
      original.variables,
      '停用不删值：照样存、照样显示，只是不参与替换',
    );
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_var_disabled',
        args: { envId: 'env-1', key: 'token', disabled: true },
      },
    ]);
  });

  it('启用一个变量：从集合里移除', async () => {
    const original = makeEnvironment({
      variables: { token: 'abc' },
      disabledVariables: ['token'],
    });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => null);

    await harness.slice.setEnvironmentVarDisabled('env-1', 'token', false);

    assert.deepEqual(harness.bag.environments[0].disabledVariables, []);
  });

  it('停用失败时恢复原集合', async () => {
    const original = makeEnvironment({
      variables: { token: 'abc' },
      disabledVariables: ['token'],
    });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => {
      throw new Error('disable failed');
    });

    await harness.slice.setEnvironmentVarDisabled('env-1', 'token', false);

    assert.deepEqual(harness.bag.environments[0].disabledVariables, ['token']);
    assert.strictEqual(harness.bag.environments[0], original);
  });
});

describe('crud-slice: 删除环境时当前环境怎么落', () => {
  /** 两个环境：dev 先创建（故是默认环境），staging 后创建。 */
  function twoEnvs() {
    return [
      makeEnvironment({ id: 'env-1', name: 'dev', createdAt: 1 }),
      makeEnvironment({ id: 'env-2', name: 'staging', createdAt: 2 }),
    ];
  }

  it('删掉当前环境时回落到这个项目的默认环境（最早创建的那个）', async () => {
    const harness = makeCrudHarness({}, twoEnvs(), [], [], 'env-2');
    installIpcMock(async () => null);

    await harness.slice.deleteEnvironment('env-2');

    assert.equal(
      harness.bag.activeEnvironmentId,
      'env-1',
      '当前环境被删掉后要落到默认环境，而不是留一个不存在的 id',
    );
    assert.deepEqual(
      harness.bag.environments.map((e) => e.id),
      ['env-1'],
      '环境列表里那个被删的确实没了',
    );
  });

  it('删掉的不是当前环境时，当前环境不动', async () => {
    const harness = makeCrudHarness({}, twoEnvs(), [], [], 'env-2');
    installIpcMock(async () => null);

    await harness.slice.deleteEnvironment('env-1');

    assert.equal(harness.bag.activeEnvironmentId, 'env-2', '当前环境不该被这次删除带走');
  });

  it('删掉当前环境且项目里再无环境时，当前环境置空', async () => {
    const harness = makeCrudHarness(
      {},
      [makeEnvironment({ id: 'env-1', createdAt: 1 })],
      [],
      [],
      'env-1',
    );
    installIpcMock(async () => null);

    await harness.slice.deleteEnvironment('env-1');

    assert.equal(harness.bag.activeEnvironmentId, '', '一个环境都不剩时当前环境置空');
  });

  it('端点行不再有环境绑定：删环境不动任何接口配置', async () => {
    const details = { 'ep-a': makeDetail({ nodeId: 'ep-a', url: '/a' }) };
    const harness = makeCrudHarness(details, twoEnvs(), [], [], 'env-1');
    installIpcMock(async () => null);

    await harness.slice.deleteEnvironment('env-1');

    assert.strictEqual(
      harness.bag.endpointDetails['ep-a'],
      details['ep-a'],
      '接口配置里没有环境这一项，删环境不该碰它',
    );
  });

  it('删除失败时环境列表与当前环境一起回滚', async () => {
    const harness = makeCrudHarness({}, twoEnvs(), [], [], 'env-2');
    installIpcMock(async () => {
      throw new Error('delete failed');
    });

    await harness.slice.deleteEnvironment('env-2');

    assert.deepEqual(
      harness.bag.environments.map((e) => e.id),
      ['env-1', 'env-2'],
      '删除失败时环境列表回到原值',
    );
    assert.equal(harness.bag.activeEnvironmentId, 'env-2', '当前环境也回到原值');
  });
});
