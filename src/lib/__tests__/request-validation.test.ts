/**
 * `src/lib/request-validation.ts` 的测试。
 *
 * 被测规则刻意很窄（「json 模式的 body 必须能解析」），所以有意思的断言是那些关于
 * 它**不该**管什么：
 *
 *   - 空的 json body 是没写完，不是格式坏了
 *   - 非 json 模式不归它管
 *   - 一次**通过**与一次失败同样有意义——它是让钉住的徽标消失的那一次，所以调用方
 *     依赖 `null` 表示「清掉它」
 *
 * 本文件保护的行为契约（只有 发送 / 保存 / 格式化 会调用它，结论被钉住直到其中之一
 * 通过）住在 HttpTester；这里断言的是底下那个纯函数。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { validateRequest } from '../request-validation';
import type { RequestBody } from '@/types/domain';

function body(content: string, mode: RequestBody['mode'] = 'json'): RequestBody {
  return { mode, content };
}

describe('validateRequest', () => {
  it('passes a well-formed json body', () => {
    assert.equal(validateRequest({ body: body('{"a":1,"b":[2,3]}') }), null);
  });

  it('reports the parser message for a malformed json body', () => {
    const issue = validateRequest({ body: body('{"a": 1,}') });
    assert.ok(issue, 'expected an issue');
    assert.equal(issue.tab, 'body');
    assert.match(issue.message, /Expected double-quoted property name/);
  });

  it('treats an empty json body as unfinished, not malformed', () => {
    // 在这里拦下，徽标就会去抱怨一个用户还没填的字段，而且根本没什么可解析的。
    assert.equal(validateRequest({ body: body('') }), null);
    assert.equal(validateRequest({ body: body('   \n ') }), null);
  });

  it('stays out of the way for non-json modes', () => {
    // `raw` 内容恰好是坏 JSON 也仍是合法的原始文本。
    assert.equal(validateRequest({ body: body('{"a":', 'raw') }), null);
    assert.equal(validateRequest({ body: body('not json at all', 'form') }), null);
    assert.equal(validateRequest({ body: body('', 'none') }), null);
  });

  it('returns null — not an issue object — so callers can clear a pinned error', () => {
    // 消掉错误的路径就是 `issue === null`。这里若返回一个 falsy 但存在的值，徽标会
    // 悄悄留在屏幕上。
    const result = validateRequest({ body: body('{"ok":true}') });
    assert.strictEqual(result, null);
  });
});

describe('validateRequest 认得带占位符的模板', () => {
  it('值位置的占位符不算 JSON 坏了', () => {
    // 动态变量的用法就是这一种：值位置不带引号，模板阶段本来就不是合法 JSON。
    assert.equal(validateRequest({ body: body('{"sns": {{sns}}}') }), null);
    assert.equal(validateRequest({ body: body('{"a": {{x}}, "b": [1,2]}') }), null);
    assert.equal(validateRequest({ body: body('{{whole}}') }), null);
  });

  it('键位置的占位符也不算 JSON 坏了', () => {
    // 键位置有两种写法：带引号的（本来就是合法 JSON）与不带引号的（需要换成带引号的
    // token 才合法）。
    assert.equal(validateRequest({ body: body('{"{{key}}": 1}') }), null);
    assert.equal(validateRequest({ body: body('{ {{key}}: 1 }') }), null);
  });

  it('字符串里的占位符不算 JSON 坏了', () => {
    assert.equal(validateRequest({ body: body('{"a": "x{{v}}y"}') }), null);
  });

  it('占位符不认得的名字照旧算非法（替换原语也只认 \\w+）', () => {
    // `{{my-var}}` 在发送时不会被替换，故它照旧是「非法的 JSON」。
    const issue = validateRequest({ body: body('{"a": {{my-var}}}') });
    assert.ok(issue, 'expected an issue');
    assert.equal(issue.tab, 'body');
  });

  it('占位符之外的坏 JSON 照旧被报出来', () => {
    const issue = validateRequest({ body: body('{"a": {{x}},}') });
    assert.ok(issue, 'expected an issue');
    assert.equal(issue.tab, 'body');
  });
});
