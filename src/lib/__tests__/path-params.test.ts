/**
 * 路径占位符的识别范围与替换规则。
 *
 * 判据来自共享样本 `fixtures/path-params-parity.json`，Rust 那一侧
 * （`crates/simple-post-core/src/path_params_test.rs`）读同一份文件。两份实现各写
 * 一遍规则，只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了一边就没
 * 人提醒另一边。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { applyPathParamValue, pathPlaceholders, substitutePathParams } from '../path-params';

interface ParityCase {
  name: string;
  url: string;
  /** 这段 URL 里认得出的占位符名，按出现顺序。 */
  placeholders: string[];
  /** 「占位符名 → 取到的值」。空列表表示没有值可取。 */
  values: Array<[string, string]>;
  /** 替换之后的 URL。 */
  expectedUrl: string;
  /** 取不到值的占位符名。 */
  expectedMissing: string[];
}

const FIXTURE: { cases: ParityCase[] } = JSON.parse(
  readFileSync(new URL('../../../fixtures/path-params-parity.json', import.meta.url), 'utf8'),
);

describe('路径占位符的识别与替换与 Rust 侧一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      assert.deepEqual(pathPlaceholders(c.url), c.placeholders, '占位符名单');

      const { url, missing } = substitutePathParams(c.url, c.values);
      assert.equal(url, c.expectedUrl, '替换后的 URL');
      // 比的是**集合**而非顺序：两侧收集缺值的次序各自是本地的，契约只承诺
      // 「哪些占位符没取到值」。
      assert.deepEqual([...missing].sort(), [...c.expectedMissing].sort(), '缺值的占位符');
    });
  }
});

/**
 * 样本本身得覆盖得住「识别范围」这半边规则。
 *
 * 替换那半边由 `fixtures/resolve-parity.json` 覆盖（那边比的是整条请求的装配结果），
 * 这边比的是「哪些 `{…}` 算占位符」——两条规则各有一份样本，改了一边就没人提醒另一边。
 */
describe('样本覆盖住了识别范围', () => {
  const names = FIXTURE.cases.map((c) => c.name);

  for (const needed of [
    '路径里的一对大括号就是一个占位符',
    '双大括号是环境变量的写法，不是路径占位符',
    '查询串里的占位符不是路径占位符',
    '主机名里的占位符不是路径占位符',
    '未闭合的大括号不算占位符',
    '名字为空不算占位符',
    '名字里有空格不算占位符',
    '名字里有制表符不算占位符',
    '名字里有不换行空格不算占位符',
    '名字里有 NEL 不算占位符',
    '名字里有斜杠不算占位符',
    '名字里有与号不算占位符',
    'U+FEFF 不在 Unicode 空白集合里，两侧都把它当名字的一部分',
  ]) {
    it(`样本里有「${needed}」这条规则`, () => {
      assert.ok(names.includes(needed), '样本里缺了这条识别规则，两侧就可能在这里分叉');
    });
  }
});

/**
 * 空白集合的差集在两侧的判据里恰好相反，故这两条断言写在两边。
 *
 * Rust 的 `char::is_whitespace` 含 U+0085（NEL）不含 U+FEFF；JS 的 `\s` 含 U+FEFF
 * 不含 U+0085。两边用不同的集合，同一段 URL 在界面与 MCP 侧就会认出不同的占位符
 * 名单。Rust 那一侧写了同款断言。
 */
describe('写一个占位符的值', () => {
  const item = (id: string, key: string, value: string) => ({
    id,
    key,
    value,
    enabled: true,
  });

  it('占位符第一次被填值时新建一条', () => {
    // 新建接口的路径参数在存储里没有条目。若这里只更新已有条目，用户的按键会被
    // 回滚，而界面上看不出任何异常 —— 那一行的输入框永远写不进东西。
    const next = applyPathParamValue([], ['id'], 'id', '42');
    assert.equal(next.length, 1);
    assert.equal(next[0].key, 'id');
    assert.equal(next[0].value, '42');
    assert.ok(next[0].id, '新建的条目要有 id');
    assert.equal(next[0].enabled, true);
  });

  it('只改被改的那一个占位符，其余的值不动', () => {
    const items = [item('a', 'id', '42'), item('b', 'postId', '7')];
    const next = applyPathParamValue(items, ['id', 'postId'], 'postId', '9');
    assert.deepEqual(
      next.map((it) => [it.key, it.value]),
      [
        ['id', '42'],
        ['postId', '9'],
      ],
      '改 postId 不该顺手改掉 id 的值',
    );
  });

  it('列表按 URL 里的占位符顺序重排', () => {
    const items = [item('a', 'postId', '7'), item('b', 'id', '42')];
    const next = applyPathParamValue(items, ['id', 'postId'], 'id', '43');
    assert.deepEqual(
      next.map((it) => it.key),
      ['id', 'postId'],
    );
  });

  it('没填过值的占位符不进列表', () => {
    // 存储里只有填过值的占位符 —— 改 URL 本身不该把标签弄脏。
    const items = [item('a', 'id', '42')];
    const next = applyPathParamValue(items, ['id', 'postId'], 'id', '43');
    assert.deepEqual(
      next.map((it) => it.key),
      ['id'],
    );
  });

  it('值被清空时条目删掉', () => {
    // 「只有填过值的占位符才进存储」：空值留在存储里，「有没有填」这件事就在两处
    // 说法不一了。
    const items = [item('a', 'id', '42')];
    const next = applyPathParamValue(items, ['id'], 'id', '');
    assert.deepEqual(next, []);
  });

  it('落单的存有条目保留在末尾', () => {
    // 用户改过占位符的名字，值还在，只是暂时没有对应的位置。静默丢掉的话那些值
    // 就再也找不回来了。
    const items = [item('a', 'id', '42'), item('b', 'oldName', 'x')];
    const next = applyPathParamValue(items, ['id'], 'id', '43');
    assert.deepEqual(
      next.map((it) => [it.key, it.value]),
      [
        ['id', '43'],
        ['oldName', 'x'],
      ],
    );
  });
});

describe('空白集合是 Unicode 的 White_Space 属性', () => {
  it('U+0085 是空白，名字里带它就不是一个合法的占位符名', () => {
    assert.deepEqual(pathPlaceholders('/users/{a\u0085b}'), []);
  });

  it('U+FEFF 不是空白，名字里带它仍然是一个占位符', () => {
    const named = pathPlaceholders('/users/{a\ufeffb}');
    assert.equal(named.length, 1, 'U+FEFF 不是空白，名字里带它仍算占位符');
    assert.equal(named[0], 'a\ufeffb');
  });
});
