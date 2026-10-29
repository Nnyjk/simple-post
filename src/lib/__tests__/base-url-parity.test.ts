/**
 * base URL 解析与代理前缀替换的共享样本读取器。
 *
 * 判据来自共享样本 `fixtures/base-url-parity.json`，Rust 那一侧
 * （`crates/simple-post-mcp/src/variable_fill/base_url_parity_test.rs`）读同一份文件。
 * 两份实现各写一遍规则，只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了
 * 一边就没人提醒另一边。
 *
 * 读取器**拒绝不认识的键**，并且**数一遍读到的条目数**：样本的键名漂了（`cases` 改名
 * 之类）时，读到的条目数就是 0，那条测试必须因此变红，而不是静默地一条用例都不跑。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { applyProxy, resolveEndpointBaseUrl } from '../url';

interface CellCase {
  url: string;
  proxy?: string | null;
}

interface ParityCase {
  name: string;
  endpoint: { baseUrlOverride: string | null };
  env: { moduleBaseUrls: Record<string, CellCase> } | null;
  moduleId: string | null;
  expected: {
    baseUrl: string;
    proxy?: string | null;
    source: 'override' | 'module' | 'none';
  };
}

interface ProxyCase {
  name: string;
  baseUrl: string;
  proxy?: string | null;
  url: string;
  expected: string;
}

interface Fixture {
  cases: ParityCase[];
  proxyCases: ProxyCase[];
}

/** 顶层允许的键。`$comment` 是样本自己的说明文字，不是数据。 */
const TOP_KEYS = ['$comment', 'cases', 'proxyCases'];
const CASE_KEYS = ['name', 'endpoint', 'env', 'moduleId', 'expected'];
const ENDPOINT_KEYS = ['baseUrlOverride'];
const ENV_KEYS = ['moduleBaseUrls'];
const CELL_KEYS = ['url', 'proxy'];
const EXPECTED_KEYS = ['baseUrl', 'proxy', 'source'];
const PROXY_CASE_KEYS = ['name', 'baseUrl', 'proxy', 'url', 'expected'];

/**
 * 断言一个对象只带列出的那些键。少一个键不算错（可选键可以缺席），多一个键算错 ——
 * 键名漂了正是这条规则要抓的事。
 */
function assertOnlyKeys(value: unknown, allowed: readonly string[], where: string): void {
  assert.equal(typeof value, 'object', `${where} 必须是对象`);
  assert.ok(value !== null && !Array.isArray(value), `${where} 必须是对象`);
  for (const key of Object.keys(value as Record<string, unknown>)) {
    assert.ok(
      allowed.includes(key),
      `${where} 里有不认识的键「${key}」。样本的键名漂了，两侧就会读到不同的东西。`,
    );
  }
}

function readFixture(): Fixture {
  const raw: unknown = JSON.parse(
    readFileSync(new URL('../../../fixtures/base-url-parity.json', import.meta.url), 'utf8'),
  );
  assertOnlyKeys(raw, TOP_KEYS, '样本顶层');

  const top = raw as Record<string, unknown>;
  assert.ok(Array.isArray(top.cases), '样本里必须有 cases 数组');
  assert.ok(Array.isArray(top.proxyCases), '样本里必须有 proxyCases 数组');

  const cases: ParityCase[] = (top.cases as unknown[]).map((rawCase, i) => {
    const where = `cases[${i}]`;
    assertOnlyKeys(rawCase, CASE_KEYS, where);
    const c = rawCase as Record<string, unknown>;
    assert.equal(typeof c.name, 'string', `${where}.name 必须是字符串`);
    assertOnlyKeys(c.endpoint, ENDPOINT_KEYS, `${where}.endpoint`);
    const endpoint = c.endpoint as Record<string, unknown>;
    assert.ok(
      endpoint.baseUrlOverride === null || typeof endpoint.baseUrlOverride === 'string',
      `${where}.endpoint.baseUrlOverride 必须是字符串或 null`,
    );
    if (c.env !== null) {
      assertOnlyKeys(c.env, ENV_KEYS, `${where}.env`);
      const moduleBaseUrls = (c.env as Record<string, unknown>).moduleBaseUrls as Record<
        string,
        unknown
      >;
      assert.equal(typeof moduleBaseUrls, 'object', `${where}.env.moduleBaseUrls 必须是对象`);
      for (const [moduleId, cell] of Object.entries(moduleBaseUrls)) {
        assertOnlyKeys(cell, CELL_KEYS, `${where}.env.moduleBaseUrls.${moduleId}`);
        assert.equal(
          typeof (cell as Record<string, unknown>).url,
          'string',
          `${where}.env.moduleBaseUrls.${moduleId}.url 必须是字符串`,
        );
      }
    }
    assert.ok(c.moduleId === null || typeof c.moduleId === 'string', `${where}.moduleId`);
    assertOnlyKeys(c.expected, EXPECTED_KEYS, `${where}.expected`);
    return c as unknown as ParityCase;
  });

  const proxyCases: ProxyCase[] = (top.proxyCases as unknown[]).map((rawCase, i) => {
    const where = `proxyCases[${i}]`;
    assertOnlyKeys(rawCase, PROXY_CASE_KEYS, where);
    const c = rawCase as Record<string, unknown>;
    assert.equal(typeof c.name, 'string', `${where}.name 必须是字符串`);
    assert.equal(typeof c.baseUrl, 'string', `${where}.baseUrl 必须是字符串`);
    assert.equal(typeof c.url, 'string', `${where}.url 必须是字符串`);
    assert.equal(typeof c.expected, 'string', `${where}.expected 必须是字符串`);
    return c as unknown as ProxyCase;
  });

  return { cases, proxyCases };
}

const FIXTURE = readFixture();

describe('base URL 的解析与 Rust 侧一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      const resolved = resolveEndpointBaseUrl(
        { baseUrlOverride: c.endpoint.baseUrlOverride },
        c.env ? { moduleBaseUrls: c.env.moduleBaseUrls } : undefined,
        c.moduleId,
      );

      assert.equal(resolved.baseUrl, c.expected.baseUrl, '解析出的 base URL');
      // `?? null`：样本里缺这个键时，Rust 侧读成 `None`，而 `assert/strict` 区分
      // `null` 与 `undefined`——不归一的话，同一份样本在两侧的判定口径不一样。
      assert.equal(resolved.proxy, c.expected.proxy ?? null, '解析出的代理');
      assert.equal(resolved.source, c.expected.source, '哪一层选中了地址');
    });
  }
});

describe('代理前缀替换与 Rust 侧一致', () => {
  it('样本里至少有一个 proxyCase', () => {
    assert.ok(FIXTURE.proxyCases.length > 0, '样本里必须有 proxyCase，否则这条测试是空的');
  });

  for (const c of FIXTURE.proxyCases) {
    it(c.name, () => {
      assert.equal(applyProxy(c.baseUrl, c.proxy ?? null, c.url), c.expected, '替换后的地址');
    });
  }
});

/**
 * 样本本身得覆盖得住解析与替换的每一条规则。
 */
describe('样本覆盖住了每一条规则', () => {
  const names = FIXTURE.cases.map((c) => c.name);

  const needed = [
    '端点带自定义主机时用自己的地址',
    '端点的 override 是空串时表示自定义但不加前缀',
    '端点带自定义主机时不套用那一格的代理',
    '端点没给 override 时读模块在环境里的那一格',
    '那一格带代理时地址与代理一起给出',
    '两个模块的地址互不相干',
    '模块没有那一格时解析不出东西',
    '那一格是空串时解析不出东西',
    '那一格只有代理没有地址时解析不出东西',
    '模块未知时没有格子可读',
    '环境缺席时解析不出东西',
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

  it('样本里有带代理的 case 与 proxyCase', () => {
    assert.ok(
      FIXTURE.cases.some((c) => c.expected.proxy),
      '样本里没有一条带代理的 case，代理这一条规则就没人钉住',
    );
    assert.ok(
      FIXTURE.proxyCases.some((c) => c.proxy),
      '样本里没有一条带代理的 proxyCase，前缀替换就没人钉住',
    );
  });
});
