/**
 * Tests for `src/lib/request-validation.ts`.
 *
 * The rule under test is deliberately narrow ("json-mode body must
 * parse"), so the interesting assertions are the ones about what it must
 * NOT complain about:
 *
 *   - an empty json body is unfinished, not malformed
 *   - non-json modes are none of its business
 *   - a **pass** is as meaningful as a failure — it is what dismisses a
 *     pinned badge, so callers depend on `null` meaning "clear it"
 *
 * The behavioural contract this file protects (only Send / Save / 格式化
 * call it, and the verdict is pinned until one of them passes) lives in
 * HttpTester; what is asserted here is the pure function underneath.
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
    // Blocking here would make the badge complain about a field the user
    // has not filled in yet, and there is nothing to parse.
    assert.equal(validateRequest({ body: body('') }), null);
    assert.equal(validateRequest({ body: body('   \n ') }), null);
  });

  it('stays out of the way for non-json modes', () => {
    // `raw` content that happens to be broken JSON is legal as raw text.
    assert.equal(validateRequest({ body: body('{"a":', 'raw') }), null);
    assert.equal(validateRequest({ body: body('not json at all', 'form') }), null);
    assert.equal(validateRequest({ body: body('', 'none') }), null);
  });

  it('returns null — not an issue object — so callers can clear a pinned error', () => {
    // The dismissal path is `issue === null`. Returning a falsy-but-present
    // value here would silently leave the badge stuck on screen.
    const result = validateRequest({ body: body('{"ok":true}') });
    assert.strictEqual(result, null);
  });
});
