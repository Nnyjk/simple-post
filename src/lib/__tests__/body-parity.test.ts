/**
 * 请求体解析规则的一致性样本：TS 的 `src/lib/body`（桌面端）。
 *
 * 同一份样本也被 Rust 侧的
 * `crates/simple-post-core/src/body_parity_test.rs` 读。两份实现各写一遍规则（逐字段
 * 还是逐段模板化、空键与停用行算不算、百分号编码用哪套字符集、产物是文本还是字节），
 * 只有一份共享样本才能钉住它们没分叉。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { binaryContentType, resolveBody } from '../body';
import type { RequestBody } from '../../types/domain';

interface ParityCase {
  name: string;
  vars: Record<string, string>;
  body: RequestBody;
  expected: {
    text: string | null;
    base64: string | null;
    missing: string[];
  };
  /** `binary` 模式的 Content-Type。缺席即不检查这一项。 */
  expectedContentType?: string;
}

interface ParityFixture {
  cases: ParityCase[];
}

const FIXTURE: ParityFixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/body-parity.json', import.meta.url), 'utf8'),
);

describe('resolveBody 与 Rust 的 core::body::resolve_body 产出一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  for (const testCase of FIXTURE.cases) {
    it(testCase.name, () => {
      const resolved = resolveBody(testCase.body, testCase.vars);
      assert.equal(resolved.text, testCase.expected.text, '文本形态');
      assert.equal(resolved.base64, testCase.expected.base64, '二进制形态');
      assert.deepEqual(resolved.missing, testCase.expected.missing, '未解析的变量');
    });
  }
});

describe('binary 模式的 Content-Type', () => {
  for (const testCase of FIXTURE.cases) {
    if (testCase.expectedContentType === undefined) continue;
    it(testCase.name, () => {
      assert.equal(
        binaryContentType(testCase.body.content),
        testCase.expectedContentType,
        'binary 的 Content-Type',
      );
    });
  }
});

describe('样本本身是有效的', () => {
  it('样本里至少有一个 binary 用例与一个 multipart 用例', () => {
    // 没有 binary 用例时，下面那条 Content-Type 规则就没有被钉住。
    const modes = FIXTURE.cases.map((c) => c.body.mode);
    assert.ok(modes.includes('binary'), '样本里必须有一个 binary 用例');
    assert.ok(modes.includes('multipart'), '样本里必须有一个 multipart 用例');
  });
});
