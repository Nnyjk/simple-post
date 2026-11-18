/**
 * GraphQL 的请求体组装与校验文案。
 *
 * GraphQL 走 HTTP：URL、header、鉴权、变量替换与普通请求同一套规则，只有请求体是
 * 组装器生成的。这里钉住三件事：
 *
 *   1. 与 `fixtures/resolve-parity.json` 里三条 GraphQL 用例同形（后端按同一份样本实现）；
 *   2. variables 必须是 JSON **对象**，不是对象或解析失败算问题，不是异常；
 *   3. 校验阶段放行 `{{var}}` 占位符，组装阶段不放行 —— 两者都在发送前拦截，不往 React 抛；
 *   4. variables 对象的键序与后端 `serde_json` 的输出一致（排序后序列化）；
 *   5. query 与 variables 都做变量替换 —— MCP 的 `graphql_payload` 同样替换 query。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { prepareRequest, type PrepareRequestInput } from '../http-build';
import { buildGraphQlPayload, canonicalJson, graphqlProblems } from '../graphql';
import { validateRequest } from '../request-validation';
import type { Environment } from '../../types/domain';

const FIXTURE = JSON.parse(
  readFileSync(new URL('../../../fixtures/resolve-parity.json', import.meta.url), 'utf8'),
) as {
  cases: Array<{
    name: string;
    env: Record<string, string>;
    input: Omit<PrepareRequestInput, 'env'>;
    expected: { url: string; headers: Array<[string, string]>; body: string | null };
  }>;
};

const GRAPHQL_CASES = FIXTURE.cases.filter((c) => c.input.protocol === 'graphql');

function envOf(variables: Record<string, string>): Environment {
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

describe('GraphQL 与后端样本同形', () => {
  it('样本里有且只有四条 GraphQL 用例', () => {
    assert.equal(GRAPHQL_CASES.length, 4);
  });

  for (const testCase of GRAPHQL_CASES) {
    it(testCase.name, () => {
      const prepared = prepareRequest({ ...testCase.input, env: envOf(testCase.env) });
      assert.equal(prepared.method, 'POST', 'GraphQL 强制 POST');
      assert.equal(prepared.url, testCase.expected.url);
      assert.deepEqual(prepared.headers, testCase.expected.headers);
      assert.equal(prepared.body, testCase.expected.body);
    });
  }

  it('端点上存的是 GET，GraphQL 组装照样发 POST', () => {
    const prepared = prepareRequest({
      method: 'GET',
      urlPath: '/graphql',
      baseUrl: '',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '', operationName: '' },
      env: envOf({}),
    });
    assert.equal(prepared.method, 'POST');
    assert.equal(prepared.body, '{"query":"{ me }"}');
  });
});

describe('buildGraphQlPayload', () => {
  it('键序固定为 query、variables、operationName', () => {
    const payload = buildGraphQlPayload({
      query: 'query Q { me }',
      variables: '{"id": 1}',
      operationName: 'Q',
    });
    assert.deepEqual(payload, {
      ok: true,
      body: '{"query":"query Q { me }","variables":{"id":1},"operationName":"Q"}',
    });
  });

  it('variables 空白时不带该键', () => {
    const payload = buildGraphQlPayload({ query: '{ me }', variables: '   ' });
    assert.deepEqual(payload, { ok: true, body: '{"query":"{ me }"}' });
  });

  it('operationName 只有空白时不带该键', () => {
    const payload = buildGraphQlPayload({
      query: '{ me }',
      variables: '',
      operationName: '   ',
    });
    assert.deepEqual(payload, { ok: true, body: '{"query":"{ me }"}' });
  });

  it('variables 是数组、字符串、数字都算问题，不是异常', () => {
    for (const raw of ['[1,2]', '"nope"', '3', 'null']) {
      const payload = buildGraphQlPayload({ query: '{ me }', variables: raw });
      assert.equal(payload.ok, false, raw);
      if (!payload.ok) assert.match(payload.problems.join('\n'), /JSON 对象/);
    }
  });

  it('variables 不是合法 JSON 时报出解析器的原话', () => {
    const payload = buildGraphQlPayload({ query: '{ me }', variables: '{"a":}' });
    assert.equal(payload.ok, false);
    if (!payload.ok) assert.match(payload.problems.join('\n'), /不是合法 JSON/);
  });

  it('查询为空算问题', () => {
    const payload = buildGraphQlPayload({ query: '   ', variables: '' });
    assert.equal(payload.ok, false);
    if (!payload.ok) assert.deepEqual(payload.problems, ['GraphQL 查询为空']);
  });
});

describe('graphqlProblems 与校验', () => {
  it('校验放行占位符，组装不放行', () => {
    const graphql = { query: '{ me }', variables: '{"id": {{userId}}}' };
    assert.deepEqual(graphqlProblems(graphql), []);
    const payload = buildGraphQlPayload(graphql);
    assert.equal(payload.ok, false);
  });

  it('变量替换后再解析：字符串值要写在引号里', () => {
    const prepared = prepareRequest({
      method: 'POST',
      urlPath: '/graphql',
      baseUrl: '',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '{"name": "{{who}}"}' },
      env: envOf({ who: 'ada' }),
    });
    assert.equal(prepared.body, '{"query":"{ me }","variables":{"name":"ada"}}');
    assert.deepEqual(prepared.missingVars, []);
  });

  it('引用位置的变量替换成裸值，JSON 因此不合法（与后端同一规则）', () => {
    const prepared = prepareRequest({
      method: 'POST',
      urlPath: '/graphql',
      baseUrl: '',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '{"name": {{who}}}' },
      env: envOf({ who: 'ada' }),
    });
    assert.equal(prepared.body, null);
    assert.match(prepared.missingVars[0] ?? '', /不是合法 JSON/);
  });

  it('校验把问题挂在 graphql 页上，文案点名 variables', () => {
    const issue = validateRequest({
      body: { mode: 'json', content: '{"ok": 1}' },
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '[1]' },
    });
    assert.equal(issue?.tab, 'graphql');
    assert.match(issue?.message ?? '', /variables/);
  });

  it('组装失败不抛异常，问题走 missingVars 通道', () => {
    const prepared = prepareRequest({
      method: 'POST',
      urlPath: '/graphql',
      baseUrl: '',
      params: [],
      headers: [],
      body: { mode: 'none', content: '' },
      auth: { kind: 'none' },
      protocol: 'graphql',
      graphql: { query: '{ me }', variables: '{oops' },
      env: envOf({}),
    });
    assert.equal(prepared.body, null);
    assert.equal(prepared.missingVars.length, 1);
    assert.match(prepared.missingVars[0] ?? '', /不是合法 JSON/);
  });
});

describe('query 的变量替换', () => {
  const input = (query: string, env: Record<string, string>): PrepareRequestInput => ({
    method: 'POST',
    urlPath: '/graphql',
    baseUrl: '',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    protocol: 'graphql',
    graphql: { query, variables: '' },
    env: envOf(env),
  });

  it('变量在环境里缺失时 query 保持原文，缺失进 missingVars', () => {
    const prepared = prepareRequest(input('query { user(id: "{{id}}") { name } }', {}));
    const payload = JSON.parse(prepared.body ?? '{}') as { query?: string };
    assert.equal(payload.query, 'query { user(id: "{{id}}") { name } }');
    assert.deepEqual(prepared.missingVars, ['id']);
  });

  it('变量在环境里有值时 query 是替换后的文本', () => {
    const prepared = prepareRequest(input('query { user(id: "{{id}}") { name } }', { id: '42' }));
    const payload = JSON.parse(prepared.body ?? '{}') as { query?: string };
    assert.equal(payload.query, 'query { user(id: "42") { name } }');
    assert.deepEqual(prepared.missingVars, []);
  });
});

describe('variables 的键序', () => {
  it('对象键排序，与后端 serde_json 的输出一致', () => {
    assert.equal(JSON.stringify(canonicalJson({ b: 1, a: 2 })), '{"a":2,"b":1}');
  });

  it('嵌套对象与数组里的对象同样排序，数组顺序不变', () => {
    const value = { z: [{ b: 1, a: 2 }], a: { d: null, c: true } };
    assert.equal(
      JSON.stringify(canonicalJson(value)),
      '{"a":{"c":true,"d":null},"z":[{"a":2,"b":1}]}',
    );
  });

  it('数字、布尔、null 与字符串原样', () => {
    assert.equal(canonicalJson(1), 1);
    assert.equal(canonicalJson(true), true);
    assert.equal(canonicalJson(null), null);
    assert.equal(canonicalJson('x'), 'x');
  });

  it('组装的 payload 里 variables 是排好键序的那份', () => {
    const built = buildGraphQlPayload({ query: '{ me }', variables: '{"b":1,"a":2}' });
    assert.equal(built.ok, true);
    assert.equal(built.ok && built.body, '{"query":"{ me }","variables":{"a":2,"b":1}}');
  });
});
