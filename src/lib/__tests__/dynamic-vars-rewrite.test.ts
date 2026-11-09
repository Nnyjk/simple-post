/**
 * 动态变量的名字扫描必须覆盖路径改写规则的目标。
 *
 * 发送路径会对改写目标单独做一遍 `{{var}}` 替换（它进了 URL，见
 * `lib/http-build/prepare.ts`），所以只出现在目标里的动态变量**会被替换**。
 * 扫描范围与替换范围必须一致：不扫的话，那个名字永远不会被取数，替换后仍是字面量
 * `{{name}}`。
 *
 * 规则的**起点**不扫：它按原文匹配，不做变量替换，故它里面的标记永远不会被展开。
 *
 * 对应 `crates/simple-post-core/src/dynamic/names.rs` 的 `list_names`；两侧的字段名与
 * 语义必须一致。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { listDynamicNames, type DynamicTemplateParts } from '../dynamic-vars';

/** 一份最小的请求模板：除被测的那一段之外没有别的变量。 */
function parts(overrides: Partial<DynamicTemplateParts> = {}): DynamicTemplateParts {
  return {
    baseUrl: 'http://svc.internal:8080/api/a',
    url: '/orders/list',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    ...overrides,
  };
}

describe('动态变量扫描覆盖路径改写目标', () => {
  it('改写目标里的变量算被引用', () => {
    const names = listDynamicNames(
      parts({ rewrite: { from: '/api/air', to: '/api/{{token}}/air' } }),
    );
    assert.ok(
      names.includes('token'),
      `改写目标里的 {{token}} 必须被扫到，否则它永远不会被取数。扫到的是：${names.join('、')}`,
    );
  });

  it('改写规则缺席时不多出名字', () => {
    const names = listDynamicNames(parts());
    assert.deepEqual(names, [], '没有改写规则时，扫描结果里不该出现任何名字');
  });

  it('改写目标是空串时不多出名字', () => {
    const names = listDynamicNames(parts({ rewrite: { from: '/api/air', to: '' } }));
    assert.deepEqual(names, [], '改写目标是空串时，扫描结果里不该出现任何名字');
  });

  it('改写起点里的变量不算被引用', () => {
    // 起点按原文匹配，不做变量替换，故它里面的标记不会被展开，也不该去取数。
    const names = listDynamicNames(parts({ rewrite: { from: '/api/{{token}}', to: '/api/air' } }));
    assert.deepEqual(names, [], '起点里的标记不会被替换，不算被引用');
  });

  it('改写目标与模块地址里的同名变量只出现一次', () => {
    const names = listDynamicNames(
      parts({
        baseUrl: 'http://svc.internal:8080/{{token}}/api',
        rewrite: { from: '/api/air', to: '/api/{{token}}/air' },
      }),
    );
    assert.deepEqual(names, ['token'], '同一个名字出现两次时只保留一条');
  });
});
