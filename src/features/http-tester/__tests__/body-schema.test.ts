/**
 * 请求体的两份内容：`content` 是载荷，`schema` 是文档里的参数定义。
 *
 * 两者都是 JSON 对象，形状接近，于是「写接口定义」与「调试一次请求」很容易写反。
 * 这里钉住三件事：
 *
 *   1. 载荷与定义分开存放，且定义**不上线** —— schema 只进文档，不进请求。
 *   2. 定义能跟着保存走 —— 写进草稿的 schema 落库时不能被丢掉。
 *   3. 界面上的 body 编辑器确实有一块给定义 —— 否则它只能由 MCP 侧写入，
 *      用户看得见却改不了，那与「没有」对用户是同一件事。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { prepareRequest } from '@/lib/http-build';
import { draftToPatch } from '@/stores/slices/crud-helpers';
import type { RequestBody } from '@/types/domain';

const SCHEMA = '{"type":"object","properties":{"id":{"type":"string"}}}';

describe('请求体：载荷与定义分开', () => {
  it('定义不上线：发出去的 body 只有载荷', () => {
    const body: RequestBody = { mode: 'json', content: '{"id":"a1"}', schema: SCHEMA };

    const prepared = prepareRequest({
      method: 'POST',
      urlPath: '/users',
      baseUrl: '',
      params: [],
      headers: [],
      body,
      auth: { kind: 'none' },
    });

    assert.equal(prepared.body, '{"id":"a1"}');
    assert.ok(!String(prepared.body).includes('properties'), '请求体里不该出现 schema 的内容');
  });

  it('定义跟着保存走：草稿里的 schema 落进补丁', () => {
    const body: RequestBody = { mode: 'json', content: '{"id":"a1"}', schema: SCHEMA };

    const patch = draftToPatch({ body });

    assert.equal(patch.body?.schema, SCHEMA, '保存时不能把定义丢掉');
    assert.equal(patch.body?.content, '{"id":"a1"}');
  });
});

describe('请求体：界面上的定义编辑器', () => {
  // 没有 DOM 渲染测试设施，故读源码确认「定义有地方可改」这件事仍然成立。
  // 断言的是**接线**（编辑器读 `value.schema`、写回 `schema`），不是文案。
  const source = readFileSync(
    resolve(process.cwd(), 'src/features/http-tester/BodyEditor.tsx'),
    'utf8',
  );

  it('编辑器读的是 body 上的 schema', () => {
    assert.ok(
      source.includes('value.schema'),
      'BodyEditor 必须读 body.schema，否则那份定义没有地方可看',
    );
  });

  it('编辑器写回时保留载荷，只改 schema', () => {
    assert.ok(/onChange\(\{\s*\.\.\.value,\s*schema:/.test(source), '改定义不能顺手清掉载荷');
  });

  it('清空定义时写 undefined，不留一份空串', () => {
    // 空串会被存成 `Some("")`，导出 OpenAPI 时它就是一份「解析不了的 schema」，
    // 于是每次导出都报一条没意义的诊断。
    assert.ok(
      /schema:\s*schema\s*\|\|\s*undefined/.test(source),
      '清空定义要让它真的没有，而不是留一份空串',
    );
  });

  it('定义区有自己的展开开关，两块内容不会糊在一起', () => {
    assert.ok(
      source.includes('body-schema-toggle'),
      '定义区需要一个明确的开关，界面才分得清哪块是载荷、哪块是定义',
    );
  });

  it('模式切成 none 之后残留的定义仍然看得见、删得掉', () => {
    // 模式切换保留整个 body（含 schema），而 `none` 的端点在导出时没有 requestBody
    // 可引用它 —— 定义区若跟着模式一起藏起来，用户就再也删不掉那份残留。
    assert.ok(
      /value\.mode === 'none'[\s\S]{0,600}SchemaSection/.test(source),
      'none 模式下残留的 schema 要仍然给出那块编辑区',
    );
  });
});
