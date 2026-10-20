/**
 * `crud-slice` 的环境值删除契约。
 *
 * 覆盖变量、模块 BaseUrl 单元格与项目默认服务的 null 删除：删除立即移除本地键并发
 * `null`（空串是真实值，不被当成删除），失败时整体回滚，以及重命名只发一次
 * `environments_rename_var` 而非拆成删除 + 写入。乐观回滚见
 * `crud-slice-endpoint-detail.test.ts`。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import {
  makeCrudHarness,
  makeEnvironment,
  makeProject,
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

  it('模块 BaseUrl null 删除单元格并发送 null，空 URL 仍作为真实值保留', async () => {
    const original = makeEnvironment({
      moduleBaseUrls: {
        api: { 'mod-1': 'https://api.example.test', 'mod-2': 'https://api2.example.test' },
        empty: { 'mod-1': '' },
      },
    });
    const harness = makeCrudHarness({}, [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    // 删掉 mod-1 的格子，mod-2 的同服务格子必须原样保留 ——
    // 每个 (模块 × 服务) 都是独立值，没有共享。
    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'api', 'mod-1', null);
    assert.equal(
      Object.hasOwn(harness.bag.environments[0].moduleBaseUrls.api ?? {}, 'mod-1'),
      false,
      'null 应从乐观状态移除该模块的单元格',
    );
    assert.equal(
      harness.bag.environments[0].moduleBaseUrls.api?.['mod-2'],
      'https://api2.example.test',
      '清除一个模块的格子不能动到同一服务的其它模块格子',
    );
    assert.equal(harness.bag.environments[0].moduleBaseUrls.empty?.['mod-1'], '');

    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'empty', 'mod-1', '');
    assert.equal(harness.bag.environments[0].moduleBaseUrls.empty?.['mod-1'], '');
    assert.deepEqual(calls, [
      {
        cmd: 'environments_set_module_baseurl',
        args: { envId: 'env-1', defId: 'api', moduleId: 'mod-1', url: null },
      },
      {
        cmd: 'environments_set_module_baseurl',
        args: { envId: 'env-1', defId: 'empty', moduleId: 'mod-1', url: '' },
      },
    ]);
  });

  it('模块 BaseUrl 删除失败时恢复原单元格和值', async () => {
    const original = makeEnvironment({
      moduleBaseUrls: { api: { 'mod-1': 'https://api.example.test' } },
    });
    const harness = makeCrudHarness({}, [original]);
    installIpcMock(async () => {
      throw new Error('base URL delete failed');
    });

    await harness.slice.setEnvironmentModuleBaseUrl('env-1', 'api', 'mod-1', null);

    assert.deepEqual(harness.bag.environments[0].moduleBaseUrls, original.moduleBaseUrls);
    assert.strictEqual(harness.bag.environments[0], original);
  });

  it('项目默认服务：写单个 id，null 清除默认', async () => {
    const original = makeProject();
    const harness = makeCrudHarness({}, [], [], [original]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    await harness.slice.setDefaultBaseUrlDefinition('project-1', 'd-2');

    // ★ 是项目上的一个 id，不是每行上的一个标志位，所以不存在
    // 「同时有两个 ★」的中间态。
    assert.equal(harness.bag.projects[0].defaultBaseUrlDefinitionId, 'd-2');
    // 服务列表本身一个字节都不该动。
    assert.strictEqual(harness.bag.projects[0].baseUrlDefinitions, original.baseUrlDefinitions);

    await harness.slice.setDefaultBaseUrlDefinition('project-1', null);
    assert.equal(
      harness.bag.projects[0].defaultBaseUrlDefinitionId,
      null,
      'null 表示项目刻意没有默认服务，而不是"恢复某个模块的 ★"',
    );

    assert.deepEqual(calls, [
      { cmd: 'base_url_defs_set_default', args: { projectId: 'project-1', defId: 'd-2' } },
      { cmd: 'base_url_defs_set_default', args: { projectId: 'project-1', defId: null } },
    ]);
  });

  it('项目默认服务写入失败时整体回滚', async () => {
    const original = makeProject();
    const harness = makeCrudHarness({}, [], [], [original]);
    installIpcMock(async () => {
      throw new Error('set default failed');
    });

    await harness.slice.setDefaultBaseUrlDefinition('project-1', 'd-2');

    assert.equal(harness.bag.projects[0].defaultBaseUrlDefinitionId, 'd-1');
    assert.strictEqual(harness.bag.projects[0], original);
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
