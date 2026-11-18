/**
 * 查询历史抽取的行为测试：从响应历史行里认出 GraphQL 请求，去重、保持新 → 旧顺序、限量。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { pickQueryHistory } from '../query-history';

function row(body: string | null): { request: { body: string | null } } {
  return { request: { body } };
}

describe('pickQueryHistory', () => {
  it('按 query 文本去重并保持入参顺序（新 → 旧）', () => {
    const entries = [
      row(JSON.stringify({ query: '{ latest }' })),
      row(JSON.stringify({ query: '{ older }' })),
      row(JSON.stringify({ query: '{ latest }' })),
    ];
    assert.deepEqual(
      pickQueryHistory(entries).map((entry) => entry.query),
      ['{ latest }', '{ older }'],
    );
  });

  it('跳过坏 JSON 与没有 query 的 JSON', () => {
    const entries = [
      row('{"query": "{ broken"'),
      row(JSON.stringify({ filter: 'name', query: 42 })),
      row(JSON.stringify({ filter: 'name' })),
      row(JSON.stringify({ query: ['not', 'a', 'string'] })),
      row(null),
      row(JSON.stringify({ query: '{ kept }' })),
    ];
    assert.deepEqual(
      pickQueryHistory(entries).map((entry) => entry.query),
      ['{ kept }'],
    );
  });

  it('没有 request 的历史行不算', () => {
    assert.deepEqual(pickQueryHistory([{}, { request: null }]), []);
  });

  it('limit 生效', () => {
    const entries = ['a', 'b', 'c', 'd'].map((name) =>
      row(JSON.stringify({ query: `{ ${name} }` })),
    );
    assert.deepEqual(
      pickQueryHistory(entries, 2).map((entry) => entry.query),
      ['{ a }', '{ b }'],
    );
  });

  it('variables 美化输出，operationName 原样带上', () => {
    const entries = [
      row(
        JSON.stringify({
          query: 'query Q { me }',
          variables: { b: 2, a: 1 },
          operationName: 'Q',
        }),
      ),
    ];
    const [entry] = pickQueryHistory(entries);
    assert.equal(entry?.operationName, 'Q');
    assert.equal(entry?.variables, '{\n  "b": 2,\n  "a": 1\n}');
  });

  it('没有 variables 与 operationName 时返回空串', () => {
    const [entry] = pickQueryHistory([row(JSON.stringify({ query: '{ ping }' }))]);
    assert.deepEqual(entry, { query: '{ ping }', variables: '', operationName: '' });
  });

  it('variables 不是对象时按没有 variables 处理', () => {
    const [entry] = pickQueryHistory([
      row(JSON.stringify({ query: '{ ping }', variables: 'not-an-object' })),
    ]);
    assert.equal(entry?.variables, '');
  });
});
