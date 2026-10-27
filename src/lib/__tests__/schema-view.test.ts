/**
 * `src/lib/schema-view.ts` 的测试。
 *
 * 界面上的属性表完全依赖这个模块给出的结论（字段名、类型、必填、说明、枚举、默认值），
 * 故这里逐条钉住那些结论：
 *
 *   - 一份普通的对象定义能被列成字段；
 *   - 嵌套对象的子字段能被列出来；
 *   - 数组能显示元素类型；
 *   - `required` / `enum` / `default` / `description` 各归各位；
 *   - `$ref` 只显示引用目标的名字，不假装能展开；
 *   - `oneOf` 给一句「候选之一」的说明；
 *   - 解析不了与顶层不是对象，都给一句能读的结论，而不是抛异常。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { parseSchemaView } from '../schema-view';

/** 解析成功时取出结构；解析失败时让断言自己失败并显示结果。 */
function viewOf(text: string) {
  const result = parseSchemaView(text);
  assert.ok(result.ok, `这份文本本该解析成功：${JSON.stringify(result)}`);
  return result.view;
}

/** 解析失败时取出失败结论。 */
function failureOf(text: string) {
  const result = parseSchemaView(text);
  assert.ok(!result.ok, '这份文本本该解析失败');
  return result;
}

describe('parseSchemaView：一份普通的对象定义', () => {
  const schema = JSON.stringify({
    type: 'object',
    required: ['id'],
    properties: {
      id: { type: 'string', description: '用户编号' },
      age: { type: 'integer' },
    },
  });

  it('列出每一个字段', () => {
    const view = viewOf(schema);

    assert.equal(view.type, 'object');
    assert.deepEqual(
      view.fields.map((field) => field.name),
      ['id', 'age'],
    );
  });

  it('给出每个字段的类型', () => {
    const view = viewOf(schema);

    assert.equal(view.fields[0].type, 'string');
    assert.equal(view.fields[1].type, 'integer');
  });

  it('按父级的 required 列表标出必填字段', () => {
    const view = viewOf(schema);

    assert.equal(view.fields[0].required, true);
    assert.equal(view.fields[1].required, false);
  });

  it('把 description 交给说明列', () => {
    const view = viewOf(schema);

    assert.equal(view.fields[0].description, '用户编号');
    assert.equal(view.fields[1].description, undefined);
  });
});

describe('parseSchemaView：嵌套的对象', () => {
  const schema = JSON.stringify({
    type: 'object',
    properties: {
      user: {
        type: 'object',
        properties: {
          name: { type: 'string' },
          tags: { type: 'array', items: { type: 'string' } },
        },
      },
    },
  });

  it('把子字段挂在父字段的 children 上', () => {
    const view = viewOf(schema);

    assert.deepEqual(
      view.fields[0].children?.map((child) => child.name),
      ['name', 'tags'],
    );
  });

  it('没有 properties 的字段没有 children', () => {
    const view = viewOf(schema);

    assert.equal(view.fields[0].children?.[0].children, undefined);
  });

  it('没有写 type 但有 properties 时按对象处理', () => {
    const view = viewOf(JSON.stringify({ properties: { id: { type: 'string' } } }));

    assert.equal(view.type, 'object');
    assert.deepEqual(
      view.fields.map((field) => field.name),
      ['id'],
    );
  });
});

describe('parseSchemaView：数组', () => {
  it('显示元素类型', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: { tags: { type: 'array', items: { type: 'string' } } },
      }),
    );

    assert.equal(view.fields[0].type, 'array<string>');
  });

  it('元素是对象时，元素的字段挂在该字段的 children 上', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: {
          items: {
            type: 'array',
            items: { type: 'object', properties: { sku: { type: 'string' } } },
          },
        },
      }),
    );

    assert.equal(view.fields[0].type, 'array<object>');
    assert.deepEqual(
      view.fields[0].children?.map((child) => child.name),
      ['sku'],
    );
  });

  it('items 是一组候选时取第一条的类型，并说明候选的个数', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: {
          value: { type: 'array', items: [{ type: 'string' }, { type: 'number' }] },
        },
      }),
    );

    assert.equal(view.fields[0].type, 'array<string>');
    assert.match(view.fields[0].note ?? '', /元素有 2 种候选写法/);
  });

  it('没有写 type 但有 items 时按数组处理', () => {
    const view = viewOf(JSON.stringify({ items: { type: 'string' } }));

    assert.equal(view.type, 'array<string>');
  });
});

describe('parseSchemaView：枚举与默认值', () => {
  const schema = JSON.stringify({
    type: 'object',
    properties: {
      status: { type: 'string', enum: ['new', 'done'] },
      count: { type: 'integer', default: 3 },
    },
  });

  it('列出枚举的每一个取值', () => {
    const view = viewOf(schema);

    assert.deepEqual(view.fields[0].enumValues, ['"new"', '"done"']);
  });

  it('给出默认值', () => {
    const view = viewOf(schema);

    assert.equal(view.fields[1].defaultValue, '3');
  });

  it('没有 enum 与 default 时那两栏留空', () => {
    const view = viewOf(schema);

    assert.equal(view.fields[0].defaultValue, undefined);
    assert.equal(view.fields[1].enumValues, undefined);
  });
});

describe('parseSchemaView：引用与组合关键字', () => {
  it('$ref 只显示引用目标的名字，不假装能展开', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: { owner: { $ref: '#/components/schemas/User' } },
      }),
    );

    assert.equal(view.fields[0].type, 'User（引用）');
    assert.equal(view.fields[0].children, undefined);
  });

  it('oneOf 给一句「候选之一」的说明', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: {
          value: { oneOf: [{ type: 'string' }, { type: 'number' }] },
        },
      }),
    );

    assert.match(view.fields[0].note ?? '', /候选之一/);
  });

  it('oneOf 的候选全是引用时，说明里列出引用的名字', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: {
          value: {
            oneOf: [{ $ref: '#/components/schemas/A' }, { $ref: '#/components/schemas/B' }],
          },
        },
      }),
    );

    assert.match(view.fields[0].note ?? '', /候选之一：A \| B/);
  });

  it('allOf 与 anyOf 各有自己的说法', () => {
    const allOf = viewOf(
      JSON.stringify({ type: 'object', properties: { v: { allOf: [{ type: 'string' }] } } }),
    );
    const anyOf = viewOf(
      JSON.stringify({ type: 'object', properties: { v: { anyOf: [{ type: 'string' }] } } }),
    );

    assert.match(allOf.fields[0].note ?? '', /同时满足/);
    assert.match(anyOf.fields[0].note ?? '', /任一满足/);
  });
});

describe('parseSchemaView：format 与多类型', () => {
  it('format 单独给出，不混进类型里', () => {
    const view = viewOf(
      JSON.stringify({
        type: 'object',
        properties: { at: { type: 'string', format: 'date-time' } },
      }),
    );

    assert.equal(view.fields[0].type, 'string');
    assert.equal(view.fields[0].format, 'date-time');
  });

  it('type 是数组时把几个类型并排给出', () => {
    const view = viewOf(
      JSON.stringify({ type: 'object', properties: { v: { type: ['string', 'null'] } } }),
    );

    assert.equal(view.fields[0].type, 'string | null');
  });
});

describe('parseSchemaView：三种不能画成表的形态', () => {
  it('空文本是「还没写」，不是失败', () => {
    const result = parseSchemaView('   ');

    assert.ok(result.ok);
    assert.equal(result.view.empty, true);
    assert.deepEqual(result.view.fields, []);
  });

  it('解析不了时给出失败原因，而不是抛异常', () => {
    const result = failureOf('{"type": "object",');

    assert.equal(result.kind, 'parse-error');
    assert.ok(result.message.length > 0, '失败时要说清楚为什么');
  });

  it('顶层不是对象时给出一句能读的结论', () => {
    const array = failureOf('[1, 2, 3]');
    const text = failureOf('"hello"');

    assert.equal(array.kind, 'not-an-object');
    assert.match(array.message, /顶层是数组/);
    assert.equal(text.kind, 'not-an-object');
    assert.match(text.message, /顶层是字符串/);
  });

  it('顶层是一份标量定义时没有字段，但类型仍然读得出来', () => {
    const view = viewOf(JSON.stringify({ type: 'string', description: '一段文字' }));

    assert.equal(view.type, 'string');
    assert.equal(view.description, '一段文字');
    assert.deepEqual(view.fields, []);
  });
});
