/**
 * 只替换变化的那一段。
 *
 * 载荷编辑器把新文本写回 CodeMirror 时走这条路。若整篇覆盖，编辑器会重建语法树，
 * 折叠状态与选区一并丢掉 —— 而用户只是插进去一个变量。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { changedRange } from '../model/changedRange';

/** 按差异替换之后，文本应当等于 `next`。 */
function apply(prev: string, next: string): string {
  const r = changedRange(prev, next);
  return prev.slice(0, r.from) + r.insert + prev.slice(r.to);
}

describe('changedRange：只动真正变了的那一段', () => {
  it('替换一个取值时差异只覆盖那个取值', () => {
    const prev = '{"a":1,"b":2}';
    const next = '{"a":9,"b":2}';
    const r = changedRange(prev, next);

    assert.equal(r.from, 5, '差异从被替换的那个取值开始');
    assert.equal(r.to, 6, '差异到那个取值结束');
    assert.equal(r.insert, '9');
    assert.equal(apply(prev, next), next);
  });

  it('末尾追加时差异落在末尾，末尾之前那一大段不动', () => {
    const prev = '{"a":1}';
    const next = '{"a":1,"b":2}';
    const r = changedRange(prev, next);

    assert.equal(r.from, 6, '差异落在旧文本的末尾那一格之前');
    assert.equal(r.to, 6);
    assert.equal(r.insert, ',"b":2');
    assert.equal(apply(prev, next), next);
  });

  it('开头插入时差异落在开头，开头之后那一大段不动', () => {
    const prev = '{"b":2}';
    const next = '{"a":1,"b":2}';
    const r = changedRange(prev, next);

    assert.equal(r.from, 2);
    assert.equal(r.to, 2);
    assert.equal(apply(prev, next), next);
  });

  it('删掉一段时差异覆盖被删的那一段', () => {
    const prev = '{"a":1,"b":2}';
    const next = '{"a":1}';
    const r = changedRange(prev, next);

    assert.equal(r.from, 6);
    assert.equal(r.to, 12);
    assert.equal(r.insert, '');
    assert.equal(apply(prev, next), next);
  });

  it('中间一段被换掉时差异只落在中间', () => {
    const prev = '{"a":1,"b":2}';
    const next = '{"a":1,"c":3}';
    const r = changedRange(prev, next);

    assert.equal(r.from, 8);
    assert.equal(r.to, 12);
    assert.equal(r.insert, 'c":3');
    assert.equal(apply(prev, next), next);
  });

  it('两段没有共同部分时覆盖整篇', () => {
    const r = changedRange('abc', 'xyz');

    assert.equal(r.from, 0);
    assert.equal(r.to, 3);
    assert.equal(r.insert, 'xyz');
    assert.equal(apply('abc', 'xyz'), 'xyz');
  });

  it('文本没变时差异是空的', () => {
    const r = changedRange('{"a":1}', '{"a":1}');

    assert.equal(r.from, 7);
    assert.equal(r.to, 7);
    assert.equal(r.insert, '');
    assert.equal(apply('{"a":1}', '{"a":1}'), '{"a":1}');
  });

  it('空文档与有内容的文档之间也能算出正确的差异', () => {
    assert.equal(apply('', 'abc'), 'abc');
    assert.equal(apply('abc', ''), '');
  });

  it('插入变量名时差异落在变量所在的那一处', () => {
    // 用户把 1 换成 {{token}}：变的只有那一处，前后两段原样留着。
    const prev = '{"a":1}';
    const next = '{"a":{{token}}}';
    const r = changedRange(prev, next);

    assert.equal(r.from, 5);
    assert.equal(r.to, 6);
    assert.equal(r.insert, '{{token}}');
    assert.equal(apply(prev, next), next);
  });

  it('换过之后文本一定等于新的那一份', () => {
    const cases: Array<[string, string]> = [
      ['{"a":1}', '{"a":{{token}}}'],
      ['{"a":1,"b":2}', '{"a":1,"c":3}'],
      ['{"a":1,"b":2}', '{"a":1}'],
      ['{"b":2}', '{"a":1,"b":2}'],
      ['{"a":1}', '{"a":1,"b":2}'],
      ['', '{"a":1}'],
      ['{"a":1}', ''],
      ['abc', 'xyz'],
      ['{"a":1}', '{"a":1}'],
    ];
    for (const [prev, next] of cases) {
      assert.equal(apply(prev, next), next, `${prev} → ${next}`);
    }
  });

  it('载荷编辑器走的是这段差异，不是整篇覆盖', () => {
    // 这个 runner 没有 DOM，故读源码确认「接线」：编辑器用的是差异替换。
    // 整篇覆盖会让编辑器重建语法树，折叠状态与选区一并丢掉。
    const source = readFileSync(
      resolve(process.cwd(), 'src/features/http-tester/BodyEditor.tsx'),
      'utf8',
    );

    assert.ok(source.includes('changedRange('), '编辑器要用差异替换');
    assert.ok(
      !/from:\s*0,\s*to:\s*view\.state\.doc\.length/.test(source),
      '不能整篇覆盖文档：折叠状态与选区会一并丢掉',
    );
  });
});
