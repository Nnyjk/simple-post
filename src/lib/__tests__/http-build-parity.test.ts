/**
 * 两侧请求准备规则的一致性样本：TS 的 `prepareRequest`（桌面端）。
 *
 * 同一份样本也被 Rust 侧的
 * `crates/simple-post-mcp/src/variable_fill/resolve_parity_test.rs` 读。两份实现各写
 * 一遍规则（baseUrl 拼接、`{{var}}` 替换、收斜杠、query、header 排序、鉴权派生、
 * body「先判空后模板化」），只有一份共享样本才能钉住它们没分叉。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { prepareRequest, type PrepareRequestInput } from '../http-build';
import type { AuthConfig, Environment, KeyValue, RequestBody } from '../../types/domain';

interface ParityCase {
  name: string;
  env: Record<string, string>;
  input: Omit<PrepareRequestInput, 'env'>;
  expected: {
    url: string;
    headers: Array<[string, string]>;
    body: string | null;
  };
}

interface ParityFixture {
  cases: ParityCase[];
}

const FIXTURE: ParityFixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/resolve-parity.json', import.meta.url), 'utf8'),
);

function envOf(variables: Record<string, string>): Environment {
  return {
    id: 'env1',
    projectId: 'p1',
    name: 'dev',
    baseUrls: {},
    moduleBaseUrls: {},
    variables,
    disabledVariables: [],
    isActive: true,
  };
}

describe('prepareRequest 与 Rust 的 resolve_endpoint 产出一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  for (const testCase of FIXTURE.cases) {
    it(testCase.name, () => {
      const prepared = prepareRequest({
        ...testCase.input,
        env: envOf(testCase.env),
      });
      assert.equal(prepared.url, testCase.expected.url, 'URL');
      assert.deepEqual(prepared.headers, testCase.expected.headers, 'header');
      assert.equal(prepared.body, testCase.expected.body, 'body');
    });
  }
});

describe('样本本身是有效的', () => {
  it('KeyValue.id 必填，body 模式与 Content-Type 对得上', () => {
    for (const testCase of FIXTURE.cases) {
      const params = testCase.input.params as KeyValue[];
      for (const param of params) {
        assert.equal(typeof param.id, 'string', `${testCase.name}: KeyValue.id 必填`);
      }
      const body = testCase.input.body as RequestBody;
      const hasContentType = testCase.expected.headers.some(
        ([k]) => k.toLowerCase() === 'content-type',
      );
      const expectContentType = body.mode === 'json' || body.mode === 'form' || body.mode === 'raw';
      assert.equal(
        hasContentType,
        expectContentType,
        `${testCase.name}: body 模式与 Content-Type 对不上`,
      );
    }
  });

  it('鉴权与 body 的形状与领域类型一致', () => {
    const first = FIXTURE.cases[0]!;
    const auth = first.input.auth as AuthConfig;
    assert.equal(auth.kind, 'bearer');
    assert.equal(typeof auth.bearer, 'string');
  });
});
