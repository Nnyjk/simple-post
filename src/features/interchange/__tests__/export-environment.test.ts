/**
 * 导出预览：换环境之后，提示按**所选**环境算，且不重置已勾选的条目。
 *
 * 缺陷本身：预览的 `baseUrls` 是按环境解析的，而提示（「该环境下没有配置地址」）
 * 一直沿用打开弹窗时那一趟——那时用的是激活环境。用户在下拉里换了环境，提示却还
 * 按旧环境算：已经配好地址的模块被点名要求配置，真正缺地址的模块反而不被提示。
 * 说的与做的不一致，且方向是反的。
 *
 * 修法是「换环境就重取一趟预览」（复用 `interchangeExportPreview`，不新增端点），
 * 并把提示里的「该环境」换成所选环境的名字。
 *
 * 本仓库没有 React hook / 组件测试的设施，故分两层断言：
 *   - 纯函数（缺地址的模块名、环境的名字）走**行为**断言；
 *   - hook 与弹窗的接线走**源码形状**断言，与 `import-refreshes-projects.test.ts`
 *     同一手法。
 */
import { describe, it, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import type { Environment } from '@/types/domain';
import type { ExportPreview } from '@/lib/interchange-types';
import { fetchExportPreview, missingBaseUrlModules, missingEnvironmentName } from '../useTransfer';

const ENVIRONMENTS: Environment[] = [
  {
    id: 'envA',
    projectId: 'p1',
    name: '环境 A',
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    isActive: true,
  },
  {
    id: 'envB',
    projectId: 'p1',
    name: '环境 B',
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    isActive: false,
  },
];

/**
 * 一份导出预览：模块 `m` 在给定环境下有（或没有）地址。
 *
 * `baseUrl` 缺席是一个真实答案：该模块在此环境下没有配地址。
 */
function preview(environmentId: string, baseUrl?: { url: string; proxy?: string }): ExportPreview {
  return {
    groups: [
      {
        key: 'm1',
        name: '模块 m',
        endpoints: [
          {
            key: 'e1',
            name: '接口',
            method: 'GET',
            path: '/ping',
            bodyMode: 'none',
            auth: 'none',
            duplicate: false,
            diagnosticIndexes: [],
          },
        ],
        children: [],
        endpointTotal: 1,
      },
    ],
    counts: { groups: 1, endpoints: 1, duplicates: 0 },
    environmentId,
    baseUrls: [
      {
        moduleKey: 'm1',
        moduleName: '模块 m',
        ...(baseUrl ? { baseUrl } : {}),
      },
    ],
    diagnostics: [],
  };
}

interface Captured {
  url: string;
  body: Record<string, unknown>;
}

const originalFetch = globalThis.fetch;
let captured: Captured[] = [];

/**
 * 装一个按**请求里的环境 id**作答的 fetch 桩。
 *
 * 共用的那个桩（`src/lib/__tests__/tauri-commands-fetch-mock.ts`）只能回一份固定
 * 答案，而这条测试要的正是「同一个模块在不同环境下有没有地址」，故自己装一个。
 */
function installEnvAwareFetch(previews: Record<string, ExportPreview>): void {
  captured = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const body = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>;
    captured.push({ url: String(input), body });
    const key = typeof body.environmentId === 'string' ? body.environmentId : '';
    const found = previews[key];
    if (!found) {
      return new Response(JSON.stringify({ code: 'not_found', message: `没有环境 ${key}` }), {
        status: 404,
        headers: { 'content-type': 'application/json' },
      });
    }
    return new Response(JSON.stringify(found), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  }) as typeof fetch;
}

describe('导出预览：环境与提示', () => {
  beforeEach(() => {
    // 环境 A：模块 m 没有地址。环境 B：模块 m 有地址。
    installEnvAwareFetch({
      envA: preview('envA'),
      envB: preview('envB', { url: 'http://b.example' }),
    });
  });

  after(() => {
    globalThis.fetch = originalFetch;
  });

  it('切到有地址的环境后，提示不再要求配置该模块', async () => {
    // 打开弹窗那一趟：按激活环境（环境 A）解析，模块 m 缺地址，提示出现。
    const opened = await fetchExportPreview({
      projectId: 'p1',
      moduleId: 'm1',
      environmentId: 'envA',
    });
    assert.deepEqual(missingBaseUrlModules(opened), ['模块 m'], '环境 A 下模块 m 缺地址');

    // 用户把环境切到 B：重取一趟，模块 m 在 B 下有地址，提示不再点名它。
    const switched = await fetchExportPreview({
      projectId: 'p1',
      moduleId: 'm1',
      environmentId: 'envB',
    });
    assert.deepEqual(
      missingBaseUrlModules(switched),
      [],
      '环境 B 下模块 m 有地址，提示不能再要求配置它',
    );
  });

  it('切到没有地址的环境后，提示出现', async () => {
    // 反向：先在 B（有地址），再切到 A（没有地址）。
    const opened = await fetchExportPreview({
      projectId: 'p1',
      moduleId: 'm1',
      environmentId: 'envB',
    });
    assert.deepEqual(missingBaseUrlModules(opened), []);

    const switched = await fetchExportPreview({
      projectId: 'p1',
      moduleId: 'm1',
      environmentId: 'envA',
    });
    assert.deepEqual(missingBaseUrlModules(switched), ['模块 m'], '切到缺地址的环境，提示必须出现');
  });

  it('换环境会重取一趟预览：请求里带的是所选环境', async () => {
    await fetchExportPreview({ projectId: 'p1', moduleId: 'm1', environmentId: 'envB' });

    const last = captured[captured.length - 1];
    assert.equal(last?.url, '/api/interchange/export/preview');
    // 服务端按这个 id 解析 `baseUrls`；送错环境就等于没换。
    assert.equal(last?.body.environmentId, 'envB');
    assert.equal(last?.body.projectId, 'p1');
    assert.equal(last?.body.moduleId, 'm1');
  });

  it('提示按所选环境点名，不再写「该环境」', () => {
    assert.equal(missingEnvironmentName(ENVIRONMENTS, 'envB'), '环境 B');
    assert.equal(missingEnvironmentName(ENVIRONMENTS, 'envA'), '环境 A');
    // 「不选环境」那一项没有环境可点，故不给名字。
    assert.equal(missingEnvironmentName(ENVIRONMENTS, ''), null);

    const src = readFileSync(new URL('../TransferDialog.tsx', import.meta.url), 'utf8');
    assert.ok(
      !src.includes('该环境下没有配置服务 URL'),
      '提示不能再写死「该环境」——换过环境之后它指的是旧环境',
    );
    assert.ok(src.includes('missingEnvironmentName'), '提示的名字要按所选环境取');
  });

  it('重跑预览不重置已勾选的集合 / 端点', () => {
    const src = readFileSync(new URL('../useTransfer.ts', import.meta.url), 'utf8');
    const start = src.indexOf('const changeEnvironment');
    assert.ok(start >= 0, '没能定位到换环境的回调，源文件形状变了？');
    // 取到下一个回调为止：这段就是「换环境」的全部副作用。
    const end = src.indexOf('\n  const ', start + 1);
    const block = src.slice(start, end > 0 ? end : undefined);
    assert.ok(block.includes('fetchExportPreview'), '换环境必须重取一趟预览，否则提示仍按旧环境算');
    assert.ok(
      !block.includes('setSelected('),
      '重跑预览不能整份重置勾选：勾选集的成员是节点 id，换环境不产生新节点',
    );
  });

  it('换环境时丢弃过期的那一趟预览', () => {
    // 连着换两个环境时先发的那一趟可能后到，它带回来的 `baseUrls` 属于旧环境。
    const src = readFileSync(new URL('../useTransfer.ts', import.meta.url), 'utf8');
    const start = src.indexOf('const changeEnvironment');
    assert.ok(start >= 0, '没能定位到换环境的回调，源文件形状变了？');
    const end = src.indexOf('\n  const ', start + 1);
    const block = src.slice(start, end > 0 ? end : undefined);

    assert.ok(block.includes('envRequest.next()'), '每次换环境要领一个号');
    assert.ok(
      block.includes('envRequest.isCurrent(ticket)'),
      '回来时号对不上就丢弃：写进去的是旧环境的地址',
    );
    // 成功的重取不该留着上一趟的错误消息。
    assert.ok(block.includes('setError(null)'), '重取之前先清掉上一趟的错误');
  });
});
