/**
 * 动态变量的名字扫描必须覆盖代理地址。
 *
 * 发送路径会对代理地址（那一格的实际访问地址）单独做一遍 `{{var}}` 替换
 * （见 `lib/http-build/prepare.ts`），所以只出现在代理地址里的动态变量**会被替换**。
 * 扫描范围与替换范围必须一致：不扫的话，那个名字永远不会被取数，替换后仍是字面量
 * `{{name}}`。
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

describe('动态变量扫描覆盖代理地址', () => {
  it('代理地址里的变量算被引用', () => {
    const names = listDynamicNames(parts({ proxy: 'https://gw.corp.com/{{token}}/api' }));
    assert.ok(
      names.includes('token'),
      `代理地址里的 {{token}} 必须被扫到，否则它永远不会被取数。扫到的是：${names.join('、')}`,
    );
  });

  it('代理地址缺席时不多出名字', () => {
    const names = listDynamicNames(parts());
    assert.deepEqual(names, [], '没有代理地址时，扫描结果里不该出现任何名字');
  });

  it('代理地址是空串时不多出名字', () => {
    const names = listDynamicNames(parts({ proxy: '' }));
    assert.deepEqual(names, [], '代理地址是空串时，扫描结果里不该出现任何名字');
  });

  it('代理地址与模块地址里的同名变量只出现一次', () => {
    const names = listDynamicNames(
      parts({
        baseUrl: 'http://svc.internal:8080/{{token}}/api',
        proxy: 'https://gw.corp.com/{{token}}',
      }),
    );
    assert.deepEqual(names, ['token'], '同一个名字出现两次时只保留一条');
  });
});
