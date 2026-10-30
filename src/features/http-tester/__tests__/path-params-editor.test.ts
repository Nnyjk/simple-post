/**
 * 路径参数编辑器的两处界面事实。
 *
 * 渲染走 `react-dom/server`；本仓库没有 DOM 测试环境（没有 Vitest / RTL，见 AGENTS.md），
 * 且这里没有任何东西依赖 effect。
 *
 * 运行：
 *   npx tsx --test src/features/http-tester/__tests__/path-params-editor.test.ts
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import type { KeyValue } from '@/types/domain';

import { PathParamsEditor } from '../PathParamsEditor';

function render(props: { url: string; items: KeyValue[]; error?: string | null }): string {
  return renderToStaticMarkup(
    createElement(PathParamsEditor, {
      url: props.url,
      items: props.items,
      error: props.error ?? null,
      onChange: () => {},
    }),
  );
}

describe('路径参数编辑器', () => {
  it('空状态也显示发送被拦下的原因', () => {
    // 占位符只存在于模块地址时，端点自己的 URL 里没有占位符，编辑器走进空状态；
    // 那条分支曾把 `error` 整段漏掉 —— 用户被切到这一页，却看不到该去哪里改。
    const html = render({
      url: '/users',
      items: [],
      error: '路径参数 id 在模块地址里没有填值，填好才能发送',
    });
    assert.match(html, /没有填值/);
  });

  it('没有错误时那条提示不出现', () => {
    const html = render({ url: '/users', items: [], error: null });
    assert.doesNotMatch(html, /data-testid="path-params-error"/);
  });

  it('停用的路径参数那行说「已停用」，不说「已填写」', () => {
    // 导入进来的路径参数可能是停用的，而 `prepareRequest` 只认启用的那一条。
    // 那一行若显示「已填写」，用户就以为请求能发出去了。
    const html = render({
      url: '/users/{id}',
      items: [{ id: 'a', key: 'id', value: '42', enabled: false }],
      error: null,
    });
    assert.match(html, /已停用/);
    assert.doesNotMatch(html, /已填写/);
  });

  it('启用且填了值的那一行说「已填写」', () => {
    const html = render({
      url: '/users/{id}',
      items: [{ id: 'a', key: 'id', value: '42', enabled: true }],
      error: null,
    });
    assert.match(html, /已填写/);
  });
});
