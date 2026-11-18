/**
 * introspection 解析与光标补全的行为测试。样本是手写的一份小 schema：一个 Query 根、一个
 * Mutation 根、一个 OBJECT 子类型、一个 INTERFACE、一个 ENUM、一个 INPUT_OBJECT、一个
 * UNION 与两个标量 —— 足够区分「哪类名字该出现在哪个位置」。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { completionCandidates, parseIntrospection, type GraphQlSchema } from '../graphql-schema';

function ref(kind: string, name: string | null, ofType?: unknown): Record<string, unknown> {
  return ofType === undefined ? { kind, name } : { kind, name, ofType };
}

const SCHEMA_BODY = {
  data: {
    __schema: {
      queryType: { name: 'Query' },
      mutationType: { name: 'Mutation' },
      subscriptionType: null,
      types: [
        {
          kind: 'OBJECT',
          name: 'Query',
          fields: [
            { name: 'ping', args: [], type: ref('SCALAR', 'String') },
            {
              name: 'user',
              args: [{ name: 'id' }],
              type: ref('NON_NULL', null, ref('OBJECT', 'User')),
            },
            {
              name: 'users',
              args: [],
              type: ref(
                'NON_NULL',
                null,
                ref('LIST', null, ref('NON_NULL', null, ref('OBJECT', 'User'))),
              ),
            },
            {
              name: 'search',
              args: [],
              type: ref(
                'NON_NULL',
                null,
                ref('LIST', null, ref('NON_NULL', null, ref('UNION', 'SearchResult'))),
              ),
            },
          ],
        },
        {
          kind: 'OBJECT',
          name: 'User',
          fields: [
            { name: 'id', args: [], type: ref('SCALAR', 'ID') },
            { name: 'name', args: [], type: ref('SCALAR', 'String') },
            { name: 'status', args: [], type: ref('ENUM', 'Status') },
          ],
        },
        {
          kind: 'INTERFACE',
          name: 'Node',
          fields: [{ name: 'id', args: [], type: ref('SCALAR', 'ID') }],
        },
        {
          kind: 'OBJECT',
          name: 'Mutation',
          fields: [
            {
              name: 'createUser',
              args: [{ name: 'name' }, { name: 'status' }],
              type: ref('OBJECT', 'User'),
            },
          ],
        },
        {
          kind: 'ENUM',
          name: 'Status',
          enumValues: [{ name: 'ACTIVE' }, { name: 'INACTIVE' }],
        },
        { kind: 'INPUT_OBJECT', name: 'UserFilter' },
        { kind: 'SCALAR', name: 'String' },
        { kind: 'SCALAR', name: 'ID' },
        { kind: 'UNION', name: 'SearchResult' },
        { kind: 'OBJECT', name: '__Schema', fields: [] },
      ],
    },
  },
};

const SCHEMA = parseIntrospection(SCHEMA_BODY);

function schemaOrFail(): GraphQlSchema {
  assert.notEqual(SCHEMA, null, '手写样本必须能解析');
  return SCHEMA as GraphQlSchema;
}

function labelsAt(document: string, position = document.length): string[] {
  return completionCandidates(document, position, schemaOrFail()).map((entry) => entry.label);
}

describe('parseIntrospection', () => {
  it('解析字段时剥掉 ! 与 [] 包装，跳过内省类型', () => {
    const schema = schemaOrFail();
    const query = schema.types.Query;
    assert.notEqual(query, undefined);
    assert.equal(query?.fields.find((field) => field.name === 'users')?.type, 'User');
    assert.equal(query?.fields.find((field) => field.name === 'user')?.args[0], 'id');
    assert.deepEqual(schema.types.Status?.enumValues, ['ACTIVE', 'INACTIVE']);
    assert.equal(schema.types.__Schema, undefined);
    assert.equal(schema.queryType, 'Query');
    assert.equal(schema.mutationType, 'Mutation');
    assert.equal(schema.subscriptionType, null);
  });

  it('没有 data.__schema 时返回 null', () => {
    assert.equal(parseIntrospection({}), null);
  });

  it('types 不是数组时返回 null', () => {
    assert.equal(parseIntrospection({ data: { __schema: { types: 'nope' } } }), null);
  });

  it('类型的 kind 不是字符串时返回 null', () => {
    assert.equal(
      parseIntrospection({
        data: { __schema: { queryType: { name: 'Query' }, types: [{ kind: 1, name: 'X' }] } },
      }),
      null,
    );
  });

  it('非对象输入返回 null 而不抛', () => {
    assert.equal(parseIntrospection(null), null);
    assert.equal(parseIntrospection('{"data":{}}'), null);
  });
});

describe('completionCandidates', () => {
  it('顶层选择集给出 query 根的字段', () => {
    const candidates = completionCandidates('query { ', 8, schemaOrFail());
    assert.deepEqual(
      candidates.map((entry) => entry.label),
      ['ping', 'search', 'user', 'users'],
    );
    assert.equal(candidates.find((entry) => entry.label === 'ping')?.detail, 'SCALAR String');
    assert.equal(candidates.find((entry) => entry.label === 'user')?.detail, 'OBJECT User');
  });

  it('匿名顶层选择集同样用 query 根', () => {
    assert.deepEqual(labelsAt('{ '), ['ping', 'search', 'user', 'users']);
  });

  it('mutation 关键字决定根类型', () => {
    assert.deepEqual(labelsAt('mutation { '), ['createUser']);
  });

  it('内层选择集给出父字段类型的字段', () => {
    assert.deepEqual(labelsAt('query { user(id: 1) { '), ['id', 'name', 'status']);
  });

  it('标量字段之后没有子字段候选', () => {
    assert.deepEqual(labelsAt('query { ping { '), []);
  });

  it('... on 之后只给可展开的 OBJECT 与 INTERFACE', () => {
    // 根类型与 mutation 根也是 OBJECT，同样可以被展开。
    assert.deepEqual(labelsAt('query { search { ... on '), ['Mutation', 'Node', 'Query', 'User']);
  });

  it('变量定义的类型位置给所有具名类型', () => {
    const labels = labelsAt('query Q($x: ');
    assert.ok(labels.includes('Status'), '枚举类型应在候选里');
    assert.ok(labels.includes('UserFilter'), '输入类型应在候选里');
    assert.ok(labels.includes('SearchResult'), '联合类型应在候选里');
    assert.ok(labels.includes('String'), '标量应在候选里');
    assert.deepEqual(
      labels,
      [...labels].sort((a, b) => a.localeCompare(b)),
    );
  });

  it('参数括号里给出当前字段的参数名', () => {
    assert.deepEqual(labelsAt('query { user('), ['id']);
    assert.deepEqual(labelsAt('mutation { createUser('), ['name', 'status']);
  });

  it('字符串里的花括号不改变上下文', () => {
    assert.deepEqual(labelsAt('query { user(id: "{") { '), ['id', 'name', 'status']);
  });

  it('注释里的花括号不改变上下文', () => {
    assert.deepEqual(labelsAt('# {\nquery { user(id: 1) { '), ['id', 'name', 'status']);
  });

  it('光标在字符串里时不给候选', () => {
    assert.deepEqual(labelsAt('query { user(id: "abc'), []);
  });

  it('不在选择集里时不给候选', () => {
    assert.deepEqual(labelsAt('query { user(id: 1) { id } } '), []);
  });

  it('未知的父类型不猜候选', () => {
    assert.deepEqual(labelsAt('query { user(id: 1) { unknownField { '), []);
  });
});
