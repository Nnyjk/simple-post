/**
 * multipart 装配的一致性样本：TS 的 `prepareRequest`（桌面端）。
 *
 * 同一份样本也被 Rust 侧的
 * `crates/simple-post-mcp/src/mcp/http_ops/multipart_parity_test.rs` 读。两份实现各写
 * 一遍 multipart 规则（boundary 怎么生成、Content-Type 放哪、用户自己写过的
 * Content-Type 怎么处理），只有一份共享样本才能钉住它们没分叉。
 *
 * boundary 每次请求现生成，两侧各生成各的，故样本里写占位符 `<BOUNDARY>`：
 * 读的时候各自把自己那个 boundary 换成占位符再比对。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { prepareRequest } from '../http-build';
import type { AuthType, BodyMode, Environment } from '../../types/domain';

const BOUNDARY_PLACEHOLDER = '<BOUNDARY>';

interface ParityCase {
  name: string;
  env: Record<string, string>;
  input: {
    method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS';
    urlPath: string;
    baseUrl: string;
    params: Array<{ id: string; key: string; value: string; enabled: boolean }>;
    headers: Array<{ id: string; key: string; value: string; enabled: boolean }>;
    body: { mode: BodyMode; content: string };
    auth: { kind: AuthType };
  };
  expected: {
    url: string;
    headers: Array<[string, string]>;
    body: string;
  };
}

interface ParityFixture {
  cases: ParityCase[];
}

const FIXTURE: ParityFixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/multipart-parity.json', import.meta.url), 'utf8'),
);

/** 样本里的环境：变量就是 `env` 那张表，其余字段用占位值。 */
function envOf(variables: Record<string, string>): Environment {
  return {
    id: 'env1',
    projectId: 'p1',
    name: 'dev',
    moduleBaseUrls: {},
    variables,
    disabledVariables: [],
    isActive: true,
  };
}

/** 从 Content-Type 里取出这次请求的 boundary。 */
function boundaryOf(headers: Array<[string, string]>): string {
  const ct = headers.find(([k]) => k.toLowerCase() === 'content-type');
  assert.ok(ct, 'multipart 请求必须带 Content-Type');
  const m = /^multipart\/form-data; boundary=(.+)$/.exec(ct[1]);
  assert.ok(m, `不是 multipart 的 Content-Type: ${ct[1]}`);
  return m[1]!;
}

/** 把这次请求的 boundary 换成占位符，让两侧的产出可比。 */
function normalize(text: string, boundary: string): string {
  return text.split(boundary).join(BOUNDARY_PLACEHOLDER);
}

describe('prepareRequest 与 Rust 的 multipart 装配产出一致', () => {
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

      const boundary = boundaryOf(prepared.headers);
      const headers = prepared.headers.map(
        ([k, v]) => [k, normalize(v, boundary)] as [string, string],
      );
      assert.deepEqual(headers, testCase.expected.headers, 'header');

      assert.equal(
        prepared.body === null ? null : normalize(prepared.body, boundary),
        testCase.expected.body,
        'body',
      );
    });
  }
});
