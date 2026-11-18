/**
 * JSON 键值树模型的单测。
 *
 * 顶层的树视图测不了：本 runner（`tsx --test` + `node:assert`）没有 DOM。故模型把路径、
 * 摘要文本、子树 JSON 与展开摊平都做成纯函数，由这里钉住它们的输出。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  buildJsonTree,
  childPath,
  containerPreview,
  defaultExpandedPaths,
  flattenTree,
  pairText,
  parseJsonTree,
  scalarText,
  subtreeJson,
  valueText,
  type JsonTreeNode,
  type JsonValue,
} from '../json-tree';

/** 取某个路径的节点；取不到就让测试当场失败，免得后续断言对着 undefined 空转。 */
function at(root: JsonTreeNode, path: string): JsonTreeNode {
  const found = flattenTree(root, () => true).find((row) => row.node.path === path);
  assert.ok(found, `missing node at ${path}`);
  return found.node;
}

describe('JSON 路径', () => {
  it('对象用点号、数组用下标，根是 $', () => {
    const root = buildJsonTree({ data: { items: [{ id: 7 }] } });
    const id = at(root, '$.data.items[0].id');
    assert.equal(id.kind, 'scalar');
    assert.equal(id.value, 7);
  });

  it('不能写成标识符的键名加引号塞进方括号', () => {
    const root = buildJsonTree({ 'a b': 1, 'a.b': 2 });
    assert.deepEqual(
      root.children.map((n) => n.path),
      ['$["a b"]', '$["a.b"]'],
    );
  });

  it('childPath 对数组下标与键名分别给出两种写法', () => {
    assert.equal(childPath('$', 3), '$[3]');
    assert.equal(childPath('$', 'items'), '$.items');
    assert.equal(childPath('$.items', 0), '$.items[0]');
    assert.equal(childPath('$', 'a-b'), '$["a-b"]');
  });
});

describe('预览文本', () => {
  it('容器报条目数，空容器直接报括号', () => {
    assert.equal(containerPreview({ a: 1, b: 2 }), '{…} 2 项');
    assert.equal(containerPreview([1, 2, 3]), '[…] 3 项');
    assert.equal(containerPreview({}), '{}');
    assert.equal(containerPreview([]), '[]');
  });

  it('树上的容器节点带预览，标量节点不带', () => {
    const root = buildJsonTree({ a: 1, b: [true], c: {} });
    assert.equal(root.preview, '{…} 3 项');
    assert.equal(root.children[1].preview, '[…] 1 项');
    assert.equal(root.children[0].preview, null);
  });
});

describe('标量文本', () => {
  it('字符串带引号与转义，其它标量取 JSON 字面量', () => {
    assert.equal(scalarText('a"b\n'), '"a\\"b\\n"');
    assert.equal(scalarText(42), '42');
    assert.equal(scalarText(true), 'true');
    assert.equal(scalarText(null), 'null');
  });

  it('复制用的值文本对字符串去引号，容器给子树 JSON', () => {
    const root = buildJsonTree({ name: 'abc', nested: { k: 1 } });
    assert.equal(valueText(root.children[0]), 'abc');
    assert.equal(valueText(root.children[1]), '{\n  "k": 1\n}');
  });
});

describe('子树 JSON', () => {
  it('子树 JSON 只在复制时现拼，缩进 2、键序按子节点顺序', () => {
    const root = buildJsonTree({ list: [{ id: 1 }] });
    assert.equal(subtreeJson(root), '{\n  "list": [\n    {\n      "id": 1\n    }\n  ]\n}');
    assert.equal(subtreeJson(at(root, '$.list[0]')), '{\n  "id": 1\n}');
    assert.equal(subtreeJson(at(root, '$.list[0].id')), '1');
  });

  it('空容器写成 {} 与 []，数组元素不带键名前缀', () => {
    const root = buildJsonTree({ empty: {}, list: [], mixed: [1, { a: 2 }] });
    assert.equal(subtreeJson(root.children[0]), '{}');
    assert.equal(subtreeJson(root.children[1]), '[]');
    assert.equal(subtreeJson(root.children[2]), '[\n  1,\n  {\n    "a": 2\n  }\n]');
  });

  it('键值对文本对对象成员给 key: value，对数组元素给下标', () => {
    const root = buildJsonTree({ items: ['x'] });
    assert.equal(pairText(root.children[0]), 'items: [\n  "x"\n]');
    assert.equal(pairText(at(root, '$.items[0]')), '0: x');
  });
});

describe('原始 token', () => {
  it('大整数不被 double 吃掉', () => {
    const result = parseJsonTree('{"id":9007199254740993}');
    assert.equal(result.ok, true);
    if (!result.ok) return;
    const id = at(result.root, '$.id');
    // double 只能表达到 9007199254740992，故这一行只能是原文。
    assert.equal(id.scalarText, '9007199254740993');
    assert.equal(valueText(id), '9007199254740993');
    assert.equal(pairText(id), 'id: 9007199254740993');
    assert.equal(subtreeJson(result.root), '{\n  "id": 9007199254740993\n}');
  });

  it('1.0 与 1e10 保持用户写的形状', () => {
    const result = parseJsonTree('{"a":1.0,"b":1e10,"c":-0.5e-3}');
    assert.equal(result.ok, true);
    if (!result.ok) return;
    assert.equal(valueText(at(result.root, '$.a')), '1.0');
    assert.equal(valueText(at(result.root, '$.b')), '1e10');
    assert.equal(valueText(at(result.root, '$.c')), '-0.5e-3');
    assert.equal(subtreeJson(result.root), '{\n  "a": 1.0,\n  "b": 1e10,\n  "c": -0.5e-3\n}');
  });

  it('字符串仍按 JSON.stringify 转义，值文本是解码后的那一份', () => {
    const result = parseJsonTree('{"s":"a\\u00e9\\n"}');
    assert.equal(result.ok, true);
    if (!result.ok) return;
    const s = at(result.root, '$.s');
    assert.equal(s.scalarText, '"aé\\n"');
    assert.equal(valueText(s), 'aé\n');
    assert.equal(subtreeJson(result.root), '{\n  "s": "aé\\n"\n}');
  });
});

describe('自写解析器的边界', () => {
  it('结尾多余字符、非法数字与缺逗号都判失败', () => {
    for (const text of ['{} {}', '01', '[1,]', '{"a":1,}', '{"a" 1}', 'nul', '[1 2]']) {
      const result = parseJsonTree(text);
      assert.equal(result.ok, false, `${text} 应判失败`);
      assert.ok(result.ok === false && result.error.length > 0);
    }
  });

  it('合法值的边界写法照收', () => {
    for (const text of ['null', 'true', 'false', '0', '-0', '1E+2', '[]', '{}', '{"a":[]}']) {
      assert.equal(parseJsonTree(text).ok, true, `${text} 应判合法`);
    }
  });

  it('重复键名以最后一次为准，位置留在第一次出现处', () => {
    const result = parseJsonTree('{"a":1,"b":2,"a":3}');
    assert.equal(result.ok, true);
    if (!result.ok) return;
    assert.deepEqual(
      result.root.children.map((n) => n.key),
      ['a', 'b'],
    );
    assert.equal(valueText(result.root.children[0]), '3');
  });
});

describe('解析失败与空态', () => {
  it('空文本与只有空白的文本报内容为空', () => {
    for (const text of ['', '  \n\t ']) {
      const result = parseJsonTree(text);
      assert.equal(result.ok, false);
      assert.equal(result.ok === false && result.error, '内容为空');
    }
  });

  it('非法 JSON 带回解析器的原因而不是抛异常', () => {
    const result = parseJsonTree('{"a": }');
    assert.equal(result.ok, false);
    assert.ok(result.ok === false && result.error.length > 0);
  });

  it('合法 JSON 解析成以 $ 为根的树', () => {
    const result = parseJsonTree(' {"a":[1]} ');
    assert.equal(result.ok, true);
    assert.equal(result.ok === true && result.root.path, '$');
    assert.equal(result.ok === true && result.root.children[0].path, '$.a');
  });
});

describe('展开与摊平', () => {
  const root = buildJsonTree({ a: { b: { c: 1 } }, d: [1, 2] });

  it('折叠节点的子孙不出现', () => {
    const rows = flattenTree(root, (n) => n.path === '$');
    assert.deepEqual(
      rows.map((r) => r.node.path),
      ['$', '$.a', '$.d'],
    );
    assert.deepEqual(
      rows.map((r) => r.depth),
      [0, 1, 1],
    );
  });

  it('全部展开得到先父后子的行序与逐层加深的缩进', () => {
    const rows = flattenTree(root, () => true);
    assert.deepEqual(
      rows.map((r) => [r.node.path, r.depth]),
      [
        ['$', 0],
        ['$.a', 1],
        ['$.a.b', 2],
        ['$.a.b.c', 3],
        ['$.d', 1],
        ['$.d[0]', 2],
        ['$.d[1]', 2],
      ],
    );
  });

  it('默认展开只到给定深度', () => {
    assert.deepEqual(defaultExpandedPaths(root), ['$', '$.a', '$.d']);
    assert.deepEqual(defaultExpandedPaths(root, 0), ['$']);
    assert.deepEqual(defaultExpandedPaths(buildJsonTree({ x: 1 })), ['$']);
  });

  it('空对象与空数组没有子节点，也就不算可展开', () => {
    assert.deepEqual(defaultExpandedPaths(buildJsonTree({ a: {}, b: [] })), ['$']);
    assert.equal(flattenTree(buildJsonTree({ a: {} }), () => true).length, 2);
  });
});

describe('深层嵌套', () => {
  const DEPTH = 40;

  it('路径逐层拼接，摊平后层数等于深度', () => {
    let value: JsonValue = { leaf: 'end' };
    for (let i = 0; i < DEPTH; i += 1) value = { a: value };
    const root = buildJsonTree(value);
    const rows = flattenTree(root, () => true);
    assert.equal(rows.length, DEPTH + 2);
    assert.equal(rows[rows.length - 1].node.path, `$${'.a'.repeat(DEPTH)}.leaf`);
    assert.equal(rows[rows.length - 1].depth, DEPTH + 1);
  });

  it('深层节点的子树 JSON 仍是格式化文本', () => {
    const root = buildJsonTree({ a: { a: { leaf: [1, 2] } } });
    assert.equal(subtreeJson(at(root, '$.a.a.leaf')), '[\n  1,\n  2\n]');
  });
});
