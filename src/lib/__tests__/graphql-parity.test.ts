/**
 * GraphQL 组装规则的一致性样本：TS 的 `buildGraphQlPayload`（经 `prepareRequest`）。
 *
 * 同一份样本也被 Rust 侧的 `variable_fill::resolve::graphql_payload`（MCP 端）读。这里钉住
 * 的规则是：query 与 variables 都做 `{{var}}` 替换、variables 原文 trim 空不带键且非空必须是
 * JSON 对象、operationName 写进 payload 的是 trim 之后的串、`enabled:false` 的变量不进池
 * （出现在模板里就算缺失）。
 *
 * 走生产路径（`prepareRequest`）而不是直接调 `buildGraphQlPayload`：替换发生在组装之前，
 * 只测纯函数会漏掉那一步。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { prepareRequest } from '../http-build';
import type { Environment } from '../../types/domain';

interface SampleGraphql {
  query: string;
  variables?: string;
  operationName?: string;
}

interface SampleVar {
  name: string;
  value: string;
  enabled?: boolean;
}

interface GraphqlParityCase {
  name: string;
  graphql: SampleGraphql;
  vars: SampleVar[];
  expectedPayload: Record<string, unknown>;
  expectedMissing: string[];
}

interface GraphqlParityFixture {
  cases: GraphqlParityCase[];
}

/** 用例允许出现的键；多一个就说明样本换了形状而读取器没跟上。 */
const CASE_KEYS = ['name', 'graphql', 'vars', 'expectedPayload', 'expectedMissing'];

const FIXTURE: GraphqlParityFixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/graphql-parity.json', import.meta.url), 'utf8'),
);

function envOf(vars: SampleVar[]): Environment {
  const variables: Record<string, string> = {};
  for (const entry of vars) {
    // `enabled` 为假的条目按停用处理：变量池里没有它，模板里出现就是缺失。
    if (entry.enabled === false) continue;
    variables[entry.name] = entry.value;
  }
  return {
    id: 'env1',
    projectId: 'p1',
    name: 'dev',
    moduleBaseUrls: {},
    variables,
    disabledVariables: [],
    createdAt: 1,
  };
}

describe('GraphQL 组装与 Rust 的 graphql_payload 产出一致', () => {
  it('样本里恰好有 6 条用例', () => {
    // 条数写死：键名漂了或样本被改小时，这条必须变红，否则下面的循环一条都走不到。
    assert.equal(FIXTURE.cases.length, 6, '用例条数变了：键名漂了或样本被改小了');
  });

  for (const testCase of FIXTURE.cases) {
    it(testCase.name, () => {
      for (const key of Object.keys(testCase as object)) {
        assert.ok(
          CASE_KEYS.includes(key),
          `${testCase.name}：样本出现未知键「${key}」，读取器只认 ${CASE_KEYS.join('、')}`,
        );
      }

      const prepared = prepareRequest({
        method: 'GET',
        urlPath: '/graphql',
        baseUrl: '',
        params: [],
        headers: [],
        body: { mode: 'none', content: '' },
        auth: { kind: 'none' },
        protocol: 'graphql',
        graphql: {
          query: testCase.graphql.query,
          variables: testCase.graphql.variables ?? '',
          operationName: testCase.graphql.operationName,
        },
        env: envOf(testCase.vars),
      });

      assert.notEqual(prepared.body, null, `${testCase.name}：组装失败`);
      assert.deepEqual(
        JSON.parse(prepared.body ?? '') as Record<string, unknown>,
        testCase.expectedPayload,
        `${testCase.name}：payload 与样本不同`,
      );

      // 缺失变量比的是名字集合：两侧收集次序各自是本地的。
      assert.deepEqual(
        [...prepared.missingVars].sort(),
        [...testCase.expectedMissing].sort(),
        `${testCase.name}：缺失变量与样本不同`,
      );
    });
  }
});
