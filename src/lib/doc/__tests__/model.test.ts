/**
 * 文档模型的测试：分组/顺序、只读已保存、**不写值**、时间格式。
 *
 * 这些断言都落在模型上而不是 HTML 上 —— 模型是渲染的唯一输入，它干净了，渲染层就没有东西可泄漏。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildApiDocModel, docGroups, formatDocDateStamp, formatDocTimestamp } from '../model';
import { docFileName } from '../file-name';
import { detail, environment, kv, node } from './fixtures';

const AT = new Date(2026, 0, 5, 7, 8, 9);

const MODULE = node({ id: 'mod_1', kind: 'module', name: '订单服务' });

test('按 sortOrder 分组，同序按 id；接口排在子集合之前', () => {
  const nodes = [
    MODULE,
    node({ id: 'col_b', kind: 'collection', parentId: 'mod_1', name: 'B 组', sortOrder: 2 }),
    node({ id: 'col_a', kind: 'collection', parentId: 'mod_1', name: 'A 组', sortOrder: 1 }),
    node({ id: 'ep_2', kind: 'endpoint', parentId: 'mod_1', name: '第二个', sortOrder: 5 }),
    node({ id: 'ep_1', kind: 'endpoint', parentId: 'mod_1', name: '第一个', sortOrder: 1 }),
    node({ id: 'ep_3', kind: 'endpoint', parentId: 'col_a', name: '组内接口', sortOrder: 0 }),
  ];
  const details = {
    ep_1: detail({ nodeId: 'ep_1', method: 'GET', url: '/orders' }),
    ep_2: detail({ nodeId: 'ep_2', method: 'POST', url: '/orders' }),
    ep_3: detail({ nodeId: 'ep_3', method: 'DELETE', url: '/orders/{id}' }),
  };

  const model = buildApiDocModel({
    projectName: '示例项目',
    moduleId: 'mod_1',
    nodes,
    details,
    selected: new Set(['ep_1', 'ep_2', 'ep_3']),
    generatedAt: AT,
  });

  assert.equal(model.root.name, '订单服务');
  assert.deepEqual(
    model.root.endpoints.map((e) => e.name),
    ['第一个', '第二个'],
  );
  assert.deepEqual(
    model.root.children.map((g) => g.name),
    ['A 组', 'B 组'],
  );
  assert.deepEqual(
    model.root.children[0].endpoints.map((e) => e.name),
    ['组内接口'],
  );
  assert.equal(model.endpointCount, 3);
});

test('没勾的接口不进文档，未保存的接口（缺 detail）也不会凭空出现', () => {
  const nodes = [
    MODULE,
    node({ id: 'ep_1', kind: 'endpoint', parentId: 'mod_1' }),
    node({ id: 'ep_2', kind: 'endpoint', parentId: 'mod_1' }),
    node({ id: 'ep_3', kind: 'endpoint', parentId: 'mod_1' }),
  ];
  const details = {
    ep_1: detail({ nodeId: 'ep_1' }),
    // ep_3 没有 detail：视作还没落库的定义，跳过而不是渲染一张空壳。
  };

  const model = buildApiDocModel({
    projectName: '示例项目',
    moduleId: 'mod_1',
    nodes,
    details,
    selected: new Set(['ep_1', 'ep_3']),
    generatedAt: AT,
  });

  assert.deepEqual(
    model.root.endpoints.map((e) => e.id),
    ['ep_1'],
  );
});

test('绝不输出值：请求头 / 查询参数 / 路径参数 / 鉴权 / 请求体 / 环境变量', () => {
  const nodes = [MODULE, node({ id: 'ep_1', kind: 'endpoint', parentId: 'mod_1' })];
  const details = {
    ep_1: detail({
      nodeId: 'ep_1',
      params: [kv('page', 'HEADER_SECRET_QUERY', '页码')],
      pathParams: [kv('id', 'PATH_SECRET', '用户 id')],
      headers: [kv('Authorization', 'Bearer HEADER_SECRET', '访问令牌'), kv('X-Empty', '')],
      auth: {
        kind: 'apikey',
        apikey: { key: 'X-Api-Key', value: 'AUTH_SECRET', inLocation: 'header' },
      },
      body: { mode: 'json', content: '{"password":"BODY_SECRET"}', schema: '{"type":"object"}' },
      docs: '调用说明',
    }),
  };
  const model = buildApiDocModel({
    projectName: '示例项目',
    moduleId: 'mod_1',
    nodes,
    details,
    selected: new Set(['ep_1']),
    generatedAt: AT,
    environment: environment({
      name: '预发',
      variables: { token: 'ENV_SECRET', region: 'cn-north' },
      disabledVariables: ['token'],
    }),
  });

  const serialized = JSON.stringify(model);
  for (const secret of [
    'HEADER_SECRET_QUERY',
    'PATH_SECRET',
    'HEADER_SECRET',
    'AUTH_SECRET',
    'BODY_SECRET',
    'ENV_SECRET',
  ]) {
    assert.equal(serialized.includes(secret), false, `模型里不该出现 ${secret}`);
  }
  // 名字与说明留下。
  assert.deepEqual(model.root.endpoints[0].params, [{ name: 'page', description: '页码' }]);
  assert.deepEqual(
    model.root.endpoints[0].headers.map((h) => h.name),
    ['Authorization', 'X-Empty'],
  );
  assert.equal(model.root.endpoints[0].auth.kind, 'apikey');
  assert.match(model.root.endpoints[0].auth.detail ?? '', /X-Api-Key/);
  assert.equal(model.root.endpoints[0].body?.contentType, 'application/json');
  assert.deepEqual(
    model.environment?.variables.map((v) => v.name),
    ['region', 'token'],
  );
  assert.deepEqual(model.environment?.variables.find((v) => v.name === 'token')?.disabled, true);
});

test('没有请求体（none）时不带 body 字段', () => {
  const nodes = [MODULE, node({ id: 'ep_1', kind: 'endpoint', parentId: 'mod_1' })];
  const model = buildApiDocModel({
    projectName: 'p',
    moduleId: 'mod_1',
    nodes,
    details: { ep_1: detail({ nodeId: 'ep_1', body: { mode: 'none', content: 'ignored' } }) },
    selected: new Set(['ep_1']),
    generatedAt: AT,
  });
  assert.equal(model.root.endpoints[0].body, undefined);
});

test('时间是本地时区的定点格式，与语言环境无关', () => {
  assert.equal(formatDocTimestamp(AT), '2026-01-05 07:08');
  assert.equal(formatDocDateStamp(AT), '20260105');
});

test('文件名清掉 Windows 非法字符，并带 YYYYMMDD', () => {
  assert.equal(docFileName('订单/服务: v2', AT), '订单服务 v2-接口文档-20260105.html');
  assert.equal(docFileName('.. ', AT), '项目-接口文档-20260105.html');
});

test('勾选树是同一份 TransferNode 形状：METHOD 与路径取自已保存的定义', () => {
  const nodes = [
    MODULE,
    node({ id: 'col_a', kind: 'collection', parentId: 'mod_1', name: 'A 组' }),
    node({ id: 'ep_1', kind: 'endpoint', parentId: 'col_a' }),
    node({ id: 'ep_2', kind: 'endpoint', parentId: 'col_a' }),
  ];
  const details = {
    ep_1: detail({ nodeId: 'ep_1', method: 'PUT', url: '/a/{id}' }),
    ep_2: detail({ nodeId: 'ep_2', method: 'DELETE', url: '/a/{id}' }),
  };

  const groups = docGroups(nodes, details, 'mod_1');
  assert.equal(groups.length, 1);
  assert.equal(groups[0].endpointTotal, 2);
  assert.deepEqual(
    groups[0].children[0].endpoints.map((e) => [e.method, e.path]),
    [
      ['PUT', '/a/{id}'],
      ['DELETE', '/a/{id}'],
    ],
  );
});

test('勾选树不列没有已保存定义的接口，数量与导出保持一致', () => {
  const nodes = [
    MODULE,
    node({ id: 'ep_saved', kind: 'endpoint', parentId: 'mod_1', name: '有定义', sortOrder: 1 }),
    node({ id: 'ep_ghost', kind: 'endpoint', parentId: 'mod_1', name: '没定义', sortOrder: 2 }),
  ];
  const groups = docGroups(nodes, { ep_saved: detail({ nodeId: 'ep_saved' }) }, 'mod_1');
  const keys = groups[0].endpoints.map((e) => e.key);
  assert.deepEqual(keys, ['ep_saved']);
  assert.equal(groups[0].endpointTotal, 1);
});
