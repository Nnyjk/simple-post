/**
 * 动态变量的名字扫描必须与「这次请求真正发出去的路径」一致。
 *
 * 路径前缀改写会换掉端点 URL 的一段：命中时扫改写后的路径，没命中时扫原路径。改写目标里
 * 的标记因此会被扫到（它进了 URL），而没被改写掉的那一段里的标记不会被扫到（它不参与
 * 这次请求）——后者若被扫到，那个名字会去取一次没人用的数，甚至被权限门拦下来，把一个
 * 本来有效的请求挡住。
 *
 * 对应 Rust 侧 `crates/simple-post-mcp/src/mcp/http_ops/dynamic.rs` 与
 * `crates/simple-post-mcp/src/mcp/project_ops/endpoint/missing.rs`：那边也是把改写算进
 * 扫的那一份路径，再交给 `core::dynamic::list_names`。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildDynamicNames } from '../model/dynamicVars';
import type { EffectiveRequest } from '../model/effectiveRequest';

const effective: EffectiveRequest = {
  params: [],
  pathParams: [],
  headers: [],
  body: { mode: 'none', content: '' },
  auth: { kind: 'none' },
  docs: '',
  tags: [],
  preRequestScript: '',
  testScript: '',
  tls: null,
};

describe('动态变量扫描跟着改写后的路径走', () => {
  it('改写命中时，目标里的变量算被引用', () => {
    const names = buildDynamicNames('http://svc.internal:8080/api/a', '/api/air/list', effective, {
      from: '/api/air',
      to: '/api/{{token}}/air',
    });
    assert.ok(
      names.includes('token'),
      `改写目标里的 {{token}} 必须被扫到，否则它永远不会被取数。扫到的是：${names.join('、')}`,
    );
  });

  it('改写没命中时，目标里的变量不算被引用', () => {
    const names = buildDynamicNames('http://svc.internal:8080/api/a', '/orders/list', effective, {
      from: '/api/air',
      to: '/api/{{token}}/air',
    });
    assert.deepEqual(names, [], '改写没命中时那个目标不参与请求，它里面的标记不该去取数');
  });

  it('改写命中时，被换掉的那一段里的变量不算被引用', () => {
    const names = buildDynamicNames(
      'http://svc.internal:8080/api/a',
      '/api/{{gone}}/list',
      effective,
      { from: '/api/{{gone}}', to: '/api/air' },
    );
    assert.deepEqual(names, [], '起点按原文匹配，被换掉的那一段不参与这次请求');
  });

  it('改写规则缺席时不多出名字', () => {
    const names = buildDynamicNames(
      'http://svc.internal:8080/api/a',
      '/orders/list',
      effective,
      null,
    );
    assert.deepEqual(names, [], '没有改写规则时，扫描结果里不该出现任何名字');
  });
});
