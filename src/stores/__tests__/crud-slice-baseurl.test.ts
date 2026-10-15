/**
 * `crud-slice` 的 baseUrl 服务 id 契约：本地铸 id、后端采纳。
 *
 * 丢掉下发的 id 就等于让项目 ★ 与环境单元格指向一条从未落库的行，`resolveEndpointBaseUrl`
 * 于是读不到地址。`addNode` 的 id 归属见 `crud-slice-add-node.test.ts`。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import { resolveEndpointBaseUrl } from '@/lib/url';
import {
  makeCrudHarness,
  makeEnvironment,
  makeProject,
  type CapturedInvoke,
} from './crud-slice-harness';
import type { Project } from '@/types/domain';
// ============================================================
// addBaseUrlDefinition —— 渲染侧铸 id，后端采纳它
//
// 要防的 bug 是"新建的服务解析不出 URL"那个。有两处在这一行落盘之前就按服务
// id 作键——项目的 ★ 和每个 `env.moduleBaseUrls[defId][moduleId]` 单元格——
// 后端铸的 id 会让两者都指着一行从未写入的行，`resolveEndpointBaseUrl` 于是对
// 用户刚配好的服务回答 ''。所以断言终止在解析器而不是请求形状上：一个熬过往返
// 之后仍然能解析的 id，就是全部的契约。
// ============================================================

describe('crud-slice: 新建 baseUrl 服务的 id 前端铸、后端采纳', () => {
  /** 一个还没有任何服务的项目，★ 也是空的。 */
  function emptyProject(): Project {
    return makeProject({ baseUrlDefinitions: [], defaultBaseUrlDefinitionId: null });
  }

  it('本地 id 随请求下发，服务行与 ★ 留在同一个 id 上', async () => {
    const harness = makeCrudHarness({}, [makeEnvironment()], [], [emptyProject()]);
    const calls: CapturedInvoke[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'base_url_defs_create') {
        const opts = (args?.opts ?? {}) as { id?: string };
        // 修复后的契约：后端采纳调用方的 id。
        return { id: opts.id, name: String(args?.name) };
      }
      throw new Error('unexpected cmd: ' + cmd);
    });

    const defId = harness.slice.addBaseUrlDefinition('project-1', 'auth');
    assert.ok(defId, 'addBaseUrlDefinition 必须同步返回 id，设置表要拿它种单元格');

    const create = calls.find((c) => c.cmd === 'base_url_defs_create');
    assert.ok(create, '必须发出 base_url_defs_create');
    assert.equal(
      (create!.args.opts as { id?: string }).id,
      defId,
      '请求必须带上本地 id，否则 ★ 和单元格会和落库的行脱钩',
    );

    // 让 create 的 .then 落定（它把返回的行换进列表）。
    await new Promise((r) => setTimeout(r, 0));

    const project = harness.bag.projects[0];
    assert.deepEqual(
      project.baseUrlDefinitions.map((d) => d.id),
      [defId],
      '服务行的 id 必须还是那个本地 id',
    );
    assert.ok(
      project.baseUrlDefinitions.some((d) => d.id === project.defaultBaseUrlDefinitionId),
      `★ (${project.defaultBaseUrlDefinitionId}) 必须指向列表里真实存在的一行，` +
        '否则项目默认永远解析为空',
    );
  });

  it('按返回的 id 填完单元格后，解析器读得到地址（显式选择与项目默认两条路）', async () => {
    const harness = makeCrudHarness({}, [makeEnvironment()], [], [emptyProject()]);
    installIpcMock(async (cmd, args) => {
      if (cmd === 'base_url_defs_create') {
        const opts = (args?.opts ?? {}) as { id?: string };
        return { id: opts.id, name: String(args?.name) };
      }
      if (cmd === 'environments_set_module_baseurl') return null;
      throw new Error('unexpected cmd: ' + cmd);
    });

    const defId = harness.slice.addBaseUrlDefinition('project-1', 'auth');
    await harness.slice.setEnvironmentModuleBaseUrl(
      'env-1',
      defId,
      'mod_a',
      'https://api.demo.dev',
    );
    await new Promise((r) => setTimeout(r, 0));

    const project = harness.bag.projects[0];
    const env = harness.bag.environments[0];
    const none = { environmentId: null, baseUrlDefinitionId: null, baseUrlOverride: null };

    assert.equal(
      resolveEndpointBaseUrl({ ...none, baseUrlDefinitionId: defId }, env, project, 'mod_a')
        .baseUrl,
      'https://api.demo.dev',
      '显式选择该服务后必须解析出它的前置 URL',
    );
    assert.equal(
      resolveEndpointBaseUrl(none, env, project, 'mod_a').baseUrl,
      'https://api.demo.dev',
      '项目默认（★）也必须解析出同一个地址',
    );
  });

  it('后端铸了另一个 id 时，本地行回滚而不是留下一条悬空 ★', async () => {
    const harness = makeCrudHarness({}, [makeEnvironment()], [], [emptyProject()]);
    installIpcMock(async (cmd) => {
      if (cmd === 'base_url_defs_create') throw new Error('UNIQUE constraint failed');
      throw new Error('unexpected cmd: ' + cmd);
    });

    harness.slice.addBaseUrlDefinition('project-1', 'auth');
    await new Promise((r) => setTimeout(r, 0));

    const project = harness.bag.projects[0];
    assert.deepEqual(project.baseUrlDefinitions, [], '写失败时乐观行要被撤掉');
    assert.equal(project.defaultBaseUrlDefinitionId, null, '★ 不能停在一个不存在的 id 上');
  });
});
