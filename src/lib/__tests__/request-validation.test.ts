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
