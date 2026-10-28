/**
 * base URL 服务链解析的共享样本读取器。
 *
 * 判据来自共享样本 `fixtures/base-url-parity.json`，Rust 那一侧
 * （`crates/simple-post-mcp/src/variable_fill/base_url_parity_test.rs`）读同一份文件。
 * 两份实现各写一遍规则，只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了
 * 一边就没人提醒另一边。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { resolveEndpointBaseUrl } from '../url';

interface ParityCase {
  name: string;
  endpoint: {
    baseUrlDefinitionId: string | null;
    baseUrlOverride: string | null;
  };
  env: { moduleBaseUrls: Record<string, Record<string, string>> } | null;
  project: {
    baseUrlDefinitions: Array<{ id: string; name: string }>;
    defaultBaseUrlDefinitionId: string | null;
  };
  moduleId: string | null;
  expected: {
    baseUrl: string;
    source: 'override' | 'definition' | 'default' | 'none';
    definitionId: string | null;
  };
}

const FIXTURE: { cases: ParityCase[] } = JSON.parse(
  readFileSync(new URL('../../../fixtures/base-url-parity.json', import.meta.url), 'utf8'),
);

describe('base URL 的服务链解析与 Rust 侧一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      // `environmentId` 不参与解析：环境由调用方给出，故这里传 null。
      const resolved = resolveEndpointBaseUrl(
        {
          environmentId: null,
          baseUrlDefinitionId: c.endpoint.baseUrlDefinitionId,
          baseUrlOverride: c.endpoint.baseUrlOverride,
        },
        c.env ? { moduleBaseUrls: c.env.moduleBaseUrls } : undefined,
        {
          baseUrlDefinitions: c.project.baseUrlDefinitions,
          defaultBaseUrlDefinitionId: c.project.defaultBaseUrlDefinitionId,
        },
        c.moduleId,
      );

      assert.equal(resolved.baseUrl, c.expected.baseUrl, '解析出的 base URL');
      assert.equal(resolved.source, c.expected.source, '哪一行选中了服务');
      // `?? null`：样本里缺这个键时，Rust 侧读成 `None`，而 `assert/strict` 区分
      // `null` 与 `undefined`——不归一的话，同一份样本在两侧的判定口径不一样。
      assert.equal(resolved.definitionId, c.expected.definitionId ?? null, '服务 id');
    });
  }
});

/**
 * 样本本身得覆盖得住服务链的每一条规则。
 */
describe('样本覆盖住了服务链的每一条规则', () => {
  const names = FIXTURE.cases.map((c) => c.name);

  const needed = [
    '端点带自定义主机时用自己的地址',
    '端点的 override 是空串时表示自定义但不加前缀',
    '端点没给 override 时走服务链',
    '端点点名的服务项目里没有时不回退到项目星标',
    '端点点名的服务项目里没有时即使环境里有那一格也不解析',
    '端点没点名服务时用项目星标',
    '项目星标指向的服务项目里没有时解析不出东西',
    '项目没有星标且端点没点名服务时解析不出东西',
    '那一格是空串时解析不出东西',
    '服务存在但环境里没有那一格时解析不出东西',
    '端点自己点名的服务胜过项目星标',
    '同一个服务在两个模块指向两个主机',
    '模块未知时没有格子可读',
    '环境缺席时解析不出东西',
    '端点点名的服务是空串时解析不出东西',
  ];

  for (const one of needed) {
    it(`样本里有「${one}」这条规则`, () => {
      assert.ok(names.includes(one), '样本里缺了这条规则，两侧就可能在这里分叉');
    });
  }

  it('样本里没有多出未登记的规则', () => {
    // 反向也查一遍：新增一条 case 却忘了登记进上面那份名单时，样本与名单就会各说各话，
    // 而这条用例仍然全绿。
    assert.equal(
      names.length,
      needed.length,
      `样本里有未登记的 case：${names.filter((n) => !needed.includes(n)).join('、')}`,
    );
  });
});
