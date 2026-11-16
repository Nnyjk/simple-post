/**
 * 渲染层测试：同一输入同一时刻必须给出同一份字节；用户可控的一切都只以转义文本出现。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { EndpointDetail, ResourceNode, ResponseDefinition } from '@/types/domain';

import { buildApiDocModel } from '../model';
import { renderApiDocHtml } from '../render';
import { detail, environment, kv, node } from './fixtures';

const AT = new Date(2026, 0, 5, 7, 8, 9);

const MODULE = node({ id: 'mod_1', kind: 'module', name: '订单服务' });
const ENDPOINT = node({ id: 'ep_1', kind: 'endpoint', parentId: 'mod_1', name: '查询订单' });

function modelOf(
  overrides: {
    nodes?: ResourceNode[];
    details?: Record<string, EndpointDetail>;
    docs?: string;
    responses?: ResponseDefinition[];
  } = {},
) {
  return buildApiDocModel({
    projectName: '示例项目',
    moduleId: 'mod_1',
    nodes: overrides.nodes ?? [MODULE, ENDPOINT],
    details: overrides.details ?? {
      ep_1: detail({
        nodeId: 'ep_1',
        method: 'GET',
        url: '/orders/{id}',
        docs: overrides.docs,
        pathParams: [kv('id', '1', '订单 id')],
        params: [kv('page', '2', '页码')],
        headers: [kv('Authorization', 'Bearer token', '访问令牌')],
        responses: overrides.responses ?? [
          {
            id: 'r1',
            status: '200',
            description: '成功',
            contentType: 'application/json',
            schema: { type: 'object', properties: { id: { type: 'string' } } },
            example: '{"id":"1"}',
          },
        ],
      }),
    },
    selected: new Set(['ep_1']),
    generatedAt: AT,
    environment: environment({ name: '本地', variables: { region: 'cn' } }),
  });
}

test('同一模型渲染两次字节完全一致', () => {
  const model = modelOf();
  assert.equal(renderApiDocHtml(model), renderApiDocHtml(model));
  // 分别构建的等价模型也一样：渲染不吃时间、不吃随机数。
  assert.equal(renderApiDocHtml(modelOf()), renderApiDocHtml(modelOf()));
});

test('产物是自包含的：无脚本、无外链、无外部字体或图片', () => {
  const html = renderApiDocHtml(modelOf());
  assert.equal(html.startsWith('<!DOCTYPE html>'), true);
  assert.equal(/<script/i.test(html), false);
  assert.equal(/<link\b/i.test(html), false);
  assert.equal(/\ssrc=/i.test(html), false);
  assert.equal(/@import/i.test(html), false);
  assert.equal(/https?:\/\//i.test(html), false);
  // 整份产物里没有可执行属性形态，只有我们自己的标签。
  assert.equal(/<[a-z][^>]*\son[a-z]+\s*=/i.test(html), false);
});

test('接口说明里的原始 HTML 被转义，不构成标签', () => {
  const html = renderApiDocHtml(modelOf({ docs: '<script>alert(1)</script>' }));
  assert.equal(/<script/i.test(html), false);
  assert.equal(html.includes('&lt;script&gt;alert(1)&lt;/script&gt;'), true);

  const withAttr = renderApiDocHtml(
    modelOf({
      nodes: [
        MODULE,
        node({
          id: 'ep_1',
          kind: 'endpoint',
          parentId: 'mod_1',
          name: '<img src=x onerror=alert(1)>',
        }),
      ],
      details: {
        ep_1: detail({
          nodeId: 'ep_1',
          headers: [kv('x" onmouseover="alert(1)', '', '引号也要转义')],
        }),
      },
    }),
  );
  assert.equal(/<[a-z][^>]*\son[a-z]+\s*=/i.test(withAttr), false);
  assert.equal(withAttr.includes('x&quot; onmouseover=&quot;alert(1)'), true);
});

test('没有响应定义时不渲染响应表，有则列状态码、content type 与示例', () => {
  const bare = renderApiDocHtml(
    modelOf({ details: { ep_1: detail({ nodeId: 'ep_1', responses: [] }) } }),
  );
  assert.equal(bare.includes('响应定义'), false);
  assert.equal(bare.includes('class="response"'), false);

  const html = renderApiDocHtml(modelOf());
  assert.equal(html.includes('<h4>响应定义</h4>'), true);
  assert.equal(html.includes('s-2xx'), true);
  assert.equal(html.includes('application/json'), true);
  assert.equal(html.includes('&quot;type&quot;: &quot;object&quot;'), true);
  assert.equal(html.includes('&quot;id&quot;: &quot;1&quot;'), true);
});

test('目录链接与章节 id 对得上，集合层级按顺序出现', () => {
  const COL_A = node({
    id: 'col_a',
    kind: 'collection',
    parentId: 'mod_1',
    name: 'A 组',
    sortOrder: 1,
  });
  const COL_B = node({
    id: 'col_b',
    kind: 'collection',
    parentId: 'mod_1',
    name: 'B 组',
    sortOrder: 2,
  });
  const model = buildApiDocModel({
    projectName: '示例项目',
    moduleId: 'mod_1',
    nodes: [MODULE, COL_B, COL_A, ENDPOINT],
    details: { ep_1: detail({ nodeId: 'ep_1' }) },
    selected: new Set(['ep_1']),
    generatedAt: AT,
  });
  const html = renderApiDocHtml(model);

  for (const match of html.matchAll(/href="#([^"]+)"/g)) {
    assert.equal(html.includes(`id="${match[1]}"`), true, `缺少锚点 ${match[1]}`);
  }
  assert.equal(html.indexOf('A 组') < html.indexOf('B 组'), true);
  assert.equal(html.indexOf('A 组') < html.indexOf('查询订单'), true);
});

test('说明里写明数据来源与快照语义，环境变量只列名字', () => {
  const html = renderApiDocHtml(modelOf());
  assert.equal(html.includes('已保存'), true);
  assert.equal(html.includes('都不会出现在这份文件里'), true);
  assert.equal(html.includes('region'), true);
  assert.equal(html.includes('cn<'), false);
});

test('响应示例重排只动空白，超出安全整数的值不被舍入', () => {
  const html = renderApiDocHtml(
    modelOf({
      responses: [
        {
          id: 'r_1',
          status: '200',
          description: 'OK',
          contentType: 'application/json',
          example: '{"id":9007199254740993,"tags":[]}',
        },
      ],
    }),
  );
  assert.equal(html.includes('9007199254740993'), true);
  assert.equal(html.includes('9007199254740992'), false);
  assert.equal(html.includes('&quot;id&quot;: 9007199254740993'), true);
});
