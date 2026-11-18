/**
 * 响应定义这一层的纯逻辑：列表操作、草稿进补丁、页签徽标。
 *
 * 钉住三件事：
 *   1. 草稿上填的响应定义**原样**进保存补丁 —— 少了这一步，草稿端点保存后定义会消失。
 *   2. 徽标报的是条数，0 条不挂徽标。
 *   3. 上下移动只在边界内生效，到头了不给新引用（否则每次点击都多渲染一次）。
 *
 * 字段合法性不在这里测：状态码语法、重复、`contentType` 的规则只在后端
 * `simple_post_core::domain::response_problems` 判一次，界面与测试都不复刻。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { ReactElement } from 'react';

import { buildTabItems } from '../tabs/tabItems';
import {
  appendResponse,
  moveResponse,
  patchResponse,
  rekeyRows,
  removeResponse,
} from '../model/responseDefinitions';
import { draftToPatch } from '@/stores/slices/crud-helpers';
import type { ResponseDefinition } from '@/types/domain';

const DEFINED: ResponseDefinition[] = [
  { id: 'resp_a', status: '200', description: '成功', contentType: 'application/json' },
  {
    id: 'resp_b',
    status: '404',
    description: '没有这个用户',
    contentType: '',
    schema: { type: 'object' },
    example: '{"error":"not found"}',
  },
];

describe('响应定义列表的纯操作', () => {
  it('追加一条空定义，id 留空交给后端铸造', () => {
    const next = appendResponse([]);
    assert.equal(next.length, 1);
    assert.equal(next[0].id, '');
    assert.equal(next[0].status, '');
    assert.equal(next[0].description, '');
    assert.equal(next[0].contentType, 'application/json');
  });

  it('按位置改一条，其余行原样留下', () => {
    const next = patchResponse(DEFINED, 1, { status: '409' });
    assert.equal(next[1].status, '409');
    assert.equal(next[1].description, '没有这个用户');
    // 没被改的那一行还是同一个对象，没有被无谓地重建。
    assert.equal(next[0], DEFINED[0]);
  });

  it('按位置删一条', () => {
    const next = removeResponse(DEFINED, 0);
    assert.deepEqual(
      next.map((r) => r.status),
      ['404'],
    );
  });

  it('上下移动换位置，到边界原样返回', () => {
    const up = moveResponse(DEFINED, 1, -1);
    assert.deepEqual(
      up.map((r) => r.status),
      ['404', '200'],
    );
    assert.deepEqual(
      DEFINED.map((r) => r.status),
      ['200', '404'],
      '移动必须给出新数组，不能就地改草稿里那份',
    );
    assert.equal(moveResponse(DEFINED, 0, -1), DEFINED);
    assert.equal(moveResponse(DEFINED, DEFINED.length - 1, 1), DEFINED);
  });
});

describe('草稿里的响应定义进保存补丁', () => {
  it('整份原样进 patch，不复制也不改名', () => {
    const patch = draftToPatch({ responses: DEFINED });
    assert.equal(patch.responses, DEFINED);
  });

  it('没碰过的字段不进补丁', () => {
    const patch = draftToPatch({ url: '/users' });
    assert.deepEqual(patch, { url: '/users' });
    assert.equal('responses' in patch, false);
  });

  it('清空成空数组也是一次明确的改动', () => {
    const patch = draftToPatch({ responses: [] });
    assert.deepEqual(patch.responses, []);
  });
});

describe('行键', () => {
  it('新行拿到本地键，上下移动时键跟着行走', () => {
    const rows = appendResponse(appendResponse([]));
    assert.deepEqual(
      rows.map((r) => r.id),
      ['', ''],
    );

    const keys = rekeyRows([], rows);
    assert.equal(new Set(keys).size, 2, '两条新行不能共用一个键');

    // 移动后调用方把键一起换位：每一行仍带着自己那个键，卡片里的本地文本才不串。
    const moved = moveResponse(rows, 0, 1);
    const swapped = [keys[1], keys[0]];
    assert.deepEqual(rekeyRows(swapped, moved), swapped);
    assert.notDeepEqual(rekeyRows(swapped, moved), keys, '换过位就换了顺序');
  });

  it('落盘后服务端 id 取代本地键', () => {
    const keys = rekeyRows(['row-1', 'row-2'], DEFINED);
    assert.deepEqual(keys, ['resp_a', 'resp_b']);
  });
});

describe('响应的页签徽标', () => {
  const state = {
    params: 0,
    path: 0,
    headers: 0,
    body: false,
    auth: false,
    script: false,
    tls: false,
    graphql: false,
  };

  it('徽标显示定义条数，「响应」排在 Docs 与脚本之间', () => {
    const items = buildTabItems({ ...state, response: 2 });
    const ids = items.map((i) => i.id);
    assert.equal(ids.indexOf('response'), ids.indexOf('docs') + 1);
    assert.equal(ids.indexOf('response'), ids.indexOf('script') - 1);

    const badge = items.find((i) => i.id === 'response')?.badge as ReactElement<{
      children: number;
    }>;
    assert.equal(badge.props.children, 2);
    assert.equal(items.find((i) => i.id === 'response')?.label, '响应');
  });

  it('没有定义时不挂徽标', () => {
    const items = buildTabItems({ ...state, response: 0 });
    assert.equal(items.find((i) => i.id === 'response')?.badge, null);
  });

  it('GraphQL 模式下 GraphQL 页签占据 Body 的位置', () => {
    const items = buildTabItems({ ...state, response: 0, graphql: true }, true);
    const ids = items.map((i) => i.id);
    assert.equal(ids.includes('body'), false);
    assert.equal(ids.indexOf('graphql'), 3);
    assert.equal(items.find((i) => i.id === 'graphql')?.label, 'GraphQL');
  });

  it('非 GraphQL 模式不出现 GraphQL 页签', () => {
    const items = buildTabItems({ ...state, response: 0 }, false);
    assert.deepEqual(
      items.map((i) => i.id),
      ['params', 'path', 'headers', 'body', 'auth', 'docs', 'response', 'script', 'tls'],
    );
  });
});
