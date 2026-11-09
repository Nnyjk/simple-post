/**
 * base URL 解析与路径前缀改写的共享样本读取器。
 *
 * 判据来自共享样本 `fixtures/base-url-parity.json`，Rust 那一侧
 * （`crates/simple-post-mcp/src/variable_fill/base_url_parity_test.rs`）读同一份文件。
 * 两份实现各写一遍规则，只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了
 * 一边就没人提醒另一边。
 *
 * 读取器**拒绝不认识的键**、**要求必填的键齐全**，并且**数一遍读到的条目数**：样本的
 * 键名漂了（`cases` 改名之类）或样本被改小时，读到的条目数就不是写死的那个数，那条
 * 测试必须因此变红，而不是静默地一条用例都不跑。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { applyRewrite, resolveEndpointBaseUrl } from '../url';
import type { RewriteRule } from '@/types/domain';

interface CellCase {
  url: string;
  rewrite?: RewriteRule | null;
}

interface ParityCase {
  name: string;
  endpoint: { baseUrlOverride: string | null };
  env: { moduleBaseUrls: Record<string, CellCase> } | null;
  moduleId: string | null;
  expected: {
    baseUrl: string;
    rewrite?: RewriteRule | null;
    source: 'override' | 'module' | 'none';
  };
}

interface RewriteCase {
  name: string;
  url: string;
  rewrite: RewriteRule | null;
  expected: string;
}

interface Fixture {
  cases: ParityCase[];
  rewriteCases: RewriteCase[];
}

/** 顶层允许的键。`$comment` 是样本自己的说明文字，不是数据。 */
const TOP_KEYS = ['$comment', 'cases', 'rewriteCases'];
const CASE_KEYS = ['name', 'endpoint', 'env', 'moduleId', 'expected'];
const ENDPOINT_KEYS = ['baseUrlOverride'];
const ENV_KEYS = ['moduleBaseUrls'];
const CELL_KEYS = ['url', 'rewrite'];
const EXPECTED_KEYS = ['baseUrl', 'rewrite', 'source'];
const REWRITE_CASE_KEYS = ['name', 'url', 'rewrite', 'expected'];
const REWRITE_KEYS = ['from', 'to'];

/**
 * 样本里 `cases` 的确切条数。
 *
 * 断言「大于 0」钉不住任何东西：键名漂了或样本被改小，那条断言照样绿。条数写死
 * 之后，改动样本的人必须同时改这个数字，改动就会落到评审眼前。
 */
const EXPECTED_CASE_COUNT = 11;
/** 样本里 `rewriteCases` 的确切条数。 */
const EXPECTED_REWRITE_CASE_COUNT = 13;

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

/**
 * 断言一个对象带齐了要求的那些键。多一个键由 `assertOnlyKeys` 抓，少一个必填键由
 * 本函数抓——必填键缺席说明样本的形状变了，读到的就不是同一件事了。
 */
function assertHasKeys(value: unknown, required: readonly string[], where: string): void {
  assert.equal(typeof value, 'object', `${where} 必须是对象`);
  assert.ok(value !== null && !Array.isArray(value), `${where} 必须是对象`);
  for (const key of required) {
    assert.ok(
      Object.prototype.hasOwnProperty.call(value, key),
      `${where} 少了必填的键「${key}」。样本的形状变了，两侧就会读到不同的东西。`,
    );
  }
}

/** 读一条改写规则：它必须带齐 `from` 与 `to` 两个键，且都是字符串。 */
function readRewrite(value: unknown, where: string): RewriteRule {
  assertOnlyKeys(value, REWRITE_KEYS, where);
  assertHasKeys(value, REWRITE_KEYS, where);
  const rule = value as Record<string, unknown>;
  assert.equal(typeof rule.from, 'string', `${where}.from 必须是字符串`);
  assert.equal(typeof rule.to, 'string', `${where}.to 必须是字符串`);
  return { from: rule.from as string, to: rule.to as string };
}

function readFixture(): Fixture {
  const raw: unknown = JSON.parse(
    readFileSync(new URL('../../../fixtures/base-url-parity.json', import.meta.url), 'utf8'),
  );
  assertOnlyKeys(raw, TOP_KEYS, '样本顶层');

  const top = raw as Record<string, unknown>;
  assertHasKeys(top, ['cases', 'rewriteCases'], '样本顶层');
  assert.ok(Array.isArray(top.cases), '样本里必须有 cases 数组');
  assert.ok(Array.isArray(top.rewriteCases), '样本里必须有 rewriteCases 数组');

  const cases: ParityCase[] = (top.cases as unknown[]).map((rawCase, i) => {
    const where = `cases[${i}]`;
    assertOnlyKeys(rawCase, CASE_KEYS, where);
    assertHasKeys(rawCase, CASE_KEYS, where);
    const c = rawCase as Record<string, unknown>;
    assert.equal(typeof c.name, 'string', `${where}.name 必须是字符串`);
    assertOnlyKeys(c.endpoint, ENDPOINT_KEYS, `${where}.endpoint`);
    assertHasKeys(c.endpoint, ENDPOINT_KEYS, `${where}.endpoint`);
    const endpoint = c.endpoint as Record<string, unknown>;
    assert.ok(
      endpoint.baseUrlOverride === null || typeof endpoint.baseUrlOverride === 'string',
      `${where}.endpoint.baseUrlOverride 必须是字符串或 null`,
    );
    if (c.env !== null) {
      assertOnlyKeys(c.env, ENV_KEYS, `${where}.env`);
      assertHasKeys(c.env, ENV_KEYS, `${where}.env`);
      const moduleBaseUrls = (c.env as Record<string, unknown>).moduleBaseUrls as Record<
        string,
        unknown
      >;
      assert.equal(typeof moduleBaseUrls, 'object', `${where}.env.moduleBaseUrls 必须是对象`);
      for (const [moduleId, cell] of Object.entries(moduleBaseUrls)) {
        assertOnlyKeys(cell, CELL_KEYS, `${where}.env.moduleBaseUrls.${moduleId}`);
        assertHasKeys(cell, ['url'], `${where}.env.moduleBaseUrls.${moduleId}`);
        const c2 = cell as Record<string, unknown>;
        assert.equal(
          typeof c2.url,
          'string',
          `${where}.env.moduleBaseUrls.${moduleId}.url 必须是字符串`,
        );
        if (c2.rewrite !== undefined && c2.rewrite !== null) {
          readRewrite(c2.rewrite, `${where}.env.moduleBaseUrls.${moduleId}.rewrite`);
        }
      }
    }
    assert.ok(c.moduleId === null || typeof c.moduleId === 'string', `${where}.moduleId`);
    assertOnlyKeys(c.expected, EXPECTED_KEYS, `${where}.expected`);
    assertHasKeys(c.expected, EXPECTED_KEYS, `${where}.expected`);
    return c as unknown as ParityCase;
  });

  const rewriteCases: RewriteCase[] = (top.rewriteCases as unknown[]).map((rawCase, i) => {
    const where = `rewriteCases[${i}]`;
    assertOnlyKeys(rawCase, REWRITE_CASE_KEYS, where);
    assertHasKeys(rawCase, REWRITE_CASE_KEYS, where);
    const c = rawCase as Record<string, unknown>;
    assert.equal(typeof c.name, 'string', `${where}.name 必须是字符串`);
    assert.equal(typeof c.url, 'string', `${where}.url 必须是字符串`);
    assert.equal(typeof c.expected, 'string', `${where}.expected 必须是字符串`);
    const rewrite = c.rewrite === null ? null : readRewrite(c.rewrite, `${where}.rewrite`);
    return { ...(c as unknown as RewriteCase), rewrite };
  });

  return { cases, rewriteCases };
}

const FIXTURE = readFixture();

describe('base URL 的解析与 Rust 侧一致', () => {
  it('样本里恰好有 11 条解析用例', () => {
    assert.equal(
      FIXTURE.cases.length,
      EXPECTED_CASE_COUNT,
      '解析用例的条数变了：键名漂了或样本被改小，这条断言必须变红',
    );
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      const resolved = resolveEndpointBaseUrl(
        { baseUrlOverride: c.endpoint.baseUrlOverride },
        c.env ? { moduleBaseUrls: c.env.moduleBaseUrls } : undefined,
        c.moduleId,
      );

      assert.equal(resolved.baseUrl, c.expected.baseUrl, '解析出的模块地址');
      // `?? null`：样本里缺这个键时，Rust 侧读成 `None`，而 `assert/strict` 区分
      // `null` 与 `undefined`——不归一的话，同一份样本在两侧的判定口径不一样。
      assert.deepEqual(resolved.rewrite, c.expected.rewrite ?? null, '解析出的路径改写规则');
      assert.equal(resolved.source, c.expected.source, '哪一层选中了地址');
    });
  }
});

describe('路径前缀改写与 Rust 侧一致', () => {
  it('样本里恰好有 13 条改写用例', () => {
    assert.equal(
      FIXTURE.rewriteCases.length,
      EXPECTED_REWRITE_CASE_COUNT,
      '改写用例的条数变了：键名漂了或样本被改小，这条断言必须变红',
    );
  });

  for (const c of FIXTURE.rewriteCases) {
    it(c.name, () => {
      assert.equal(applyRewrite(c.url, c.rewrite), c.expected, '改写后的地址');
    });
  }
});

/**
 * 样本本身得覆盖得住解析与改写的每一条规则。
 */
describe('样本覆盖住了每一条规则', () => {
  const names = FIXTURE.cases.map((c) => c.name);

  const needed = [
    '端点带自定义主机时用自己的地址',
    '端点的 override 是空串时表示自定义但不加前缀',
    '端点带自定义主机时那一格的路径改写规则照样生效',
    '端点没给 override 时读模块在环境里的那一格',
    '那一格带改写规则时地址与规则一起给出',
    '两个模块的地址互不相干',
    '模块没有那一格时解析不出东西',
    '那一格是空串时解析不出东西',
    '那一格只有改写规则没有地址时解析不出地址',
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

  it('样本里有带改写规则的 case 与 rewriteCase', () => {
    assert.ok(
      FIXTURE.cases.some((c) => c.expected.rewrite),
      '样本里没有一条带改写规则的 case，改写规则这一条就没人钉住',
    );
    assert.ok(
      FIXTURE.rewriteCases.some((c) => c.rewrite),
      '样本里没有一条带规则的 rewriteCase，改写就没人钉住',
    );
  });
});
