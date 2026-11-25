/**
 * `ModuleBaseUrlTable` 的标记层：有序规则列表、优先级序号、拖拽手柄与行内校验都在
 * 静态标记里出现。
 *
 * 渲染走 `react-dom/server`：本仓库没有 DOM 测试环境（没有 Vitest / RTL），而这些判定全在
 * 首次渲染就能看见 —— 展开状态在 `mode="project"` 下由「有没有规则」决定，不依赖 effect。
 * 交互行为本身（提交时机、换序结果）在 `rewrite-rules.test.ts` 与 store 用例里。
 *
 * 运行：
 *   npx tsx --test src/features/settings/sections/rows/__tests__/module-base-url-table.test.ts
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import type { Environment, ResourceNode, RewriteRule } from '@/types/domain';
import { ModuleBaseUrlTable } from '../ModuleBaseUrlTable';

function node(id: string, name: string, sortOrder: number): ResourceNode {
  return {
    id,
    rootId: 'p1',
    parentId: null,
    kind: 'module',
    name,
    sortOrder,
    createdAt: 0,
    updatedAt: 0,
  };
}

function environment(moduleBaseUrls: Environment['moduleBaseUrls'] = {}): Environment {
  return {
    id: 'env1',
    projectId: 'p1',
    name: 'dev',
    moduleBaseUrls,
    variables: {},
    disabledVariables: [],
    createdAt: 0,
  };
}

function render(props: {
  env: Environment;
  modules: readonly ResourceNode[];
  mode?: 'project' | 'module';
}): string {
  return renderToStaticMarkup(
    createElement(ModuleBaseUrlTable, {
      env: props.env,
      modules: props.modules,
      mode: props.mode,
      setEnvironmentModuleBaseUrl: () => {},
    }),
  );
}

const rule = (from: string, to: string): RewriteRule => ({ from, to });

describe('模块地址表 —— 有序规则列表', () => {
  it('把两条规则按顺序渲染，并标出优先级序号', () => {
    const html = render({
      env: environment({
        m1: {
          url: '127.0.0.1:8080',
          rewrites: [rule('/api/air', '/v1/air'), rule('/api', '/v2')],
        },
      }),
      modules: [node('m1', '空气', 0)],
    });

    assert.match(html, /#1/, '第一条规则要带优先级序号');
    assert.match(html, /#2/, '第二条规则要带优先级序号');
    assert.ok(
      html.indexOf('/api/air') < html.indexOf('value="/api"'),
      '渲染顺序就是匹配优先级，起点必须按落库顺序出现',
    );
    assert.match(html, /按从上到下顺序匹配，命中首条后不再向后匹配/);
  });

  it('每条规则都有拖拽手柄与上移 / 下移 / 删除按钮', () => {
    const html = render({
      env: environment({
        m1: { url: 'http://a', rewrites: [rule('/a', '/1'), rule('/b', '/2')] },
      }),
      modules: [node('m1', '空气', 0)],
    });

    assert.match(html, /draggable="true"/);
    assert.match(html, /拖动第 1 条规则调整顺序/);
    assert.match(html, /第 1 条规则上移/);
    assert.match(html, /第 1 条规则下移/);
    assert.match(html, /删除第 2 条规则/);
    assert.match(
      html,
      /aria-label="空气 · 第 1 条规则上移"[^>]*disabled/,
      '第一条没有上移目标，按钮必须禁用',
    );
    assert.match(
      html,
      /aria-label="空气 · 第 2 条规则下移"[^>]*disabled/,
      '最后一条没有下移目标，按钮必须禁用',
    );
  });

  it('重复起点在两行上都报错，且不写到后端', () => {
    const html = render({
      env: environment({
        m1: {
          url: 'http://a',
          rewrites: [rule('/api/air', '/v1'), rule('/api/air/', '/v2')],
        },
      }),
      modules: [node('m1', '空气', 0)],
    });

    const matches = html.match(/起点「\/api\/air」重复/g) ?? [];
    assert.equal(matches.length, 2, '冲突的每一行都要就地报错，用户才知道是哪两行');
    assert.match(html, /aria-invalid="true"/);
  });

  it('被前置规则遮挡时给出可调整顺序的温和提示', () => {
    const html = render({
      env: environment({
        m1: { url: 'http://a', rewrites: [rule('/', '/gateway'), rule('/api/air', '/v1')] },
      }),
      modules: [node('m1', '空气', 0)],
    });

    assert.match(html, /前置规则 #1「\/」会先匹配这一类路径/);
    assert.doesNotMatch(html, /重复/, '遮挡不是错误，不该报重复');
  });

  it('没有规则的模块只显示入口，规则区是折叠的', () => {
    const html = render({
      env: environment({ m1: { url: 'http://a' } }),
      modules: [node('m1', '空气', 0)],
    });

    assert.match(html, /未配置/);
    assert.doesNotMatch(html, /暂无路径改写规则/, '项目视图里默认折叠，不铺开空态');
  });
});

describe('模块地址表 —— module 模式', () => {
  it('没有模块列，规则区始终展开（含空态）', () => {
    const html = render({
      env: environment({ m1: { url: 'http://a' } }),
      modules: [node('m1', '空气', 0)],
      mode: 'module',
    });

    assert.match(html, /模块地址/);
    assert.doesNotMatch(html, />模块</, 'module 模式没有模块这一列');
    assert.match(html, /暂无路径改写规则/);
    assert.match(html, /添加规则/);
  });
});
