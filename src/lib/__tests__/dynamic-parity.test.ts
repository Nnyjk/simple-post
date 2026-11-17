/**
 * 动态变量规则的跨语言样本：TS 的 `src/lib/dynamic-vars.ts`。
 *
 * 同一份样本也被 Rust 侧的
 * `crates/simple-post-core/src/dynamic/resolve_parity_test.rs` 读。两份实现各写一遍
 * 规则（变量池的合并、值的序列化、哪些名字算被引用），只有一份共享样本才能钉住它们
 * 没分叉。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { listDynamicNames, mergePool, serializeValue, type ValueMode } from '../dynamic-vars';
import type { AuthConfig, KeyValue, RequestBody } from '../../types/domain';

interface MergeCase {
  name: string;
  env: Record<string, string>;
  disabled: string[];
  dynamic: Record<string, string>;
  expected: Record<string, string>;
}

interface SerializeCase {
  name: string;
  mode: ValueMode;
  value: unknown;
  expected: string;
}

interface ScanCase {
  name: string;
  parts: {
    baseUrl: string;

    url: string;
    params: KeyValue[];
    pathParams?: KeyValue[];
    headers: KeyValue[];
    body: RequestBody;
    auth: AuthConfig;
  };
  expected: string[];
}

interface ParityFixture {
  mergeCases: MergeCase[];
  serializeCases: SerializeCase[];
  scanCases: ScanCase[];
}

const FIXTURE: ParityFixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/dynamic-parity.json', import.meta.url), 'utf8'),
);

describe('动态变量规则与 Rust 的 core::dynamic 产出一致', () => {
  it('样本的条数钉住三个读取器', () => {
    assert.equal(
      FIXTURE.mergeCases.length,
      5,
      '合并规则样本必须恰好 5 条：条数变了说明夹具动了，两侧读取器都得跟着看',
    );
    assert.equal(
      FIXTURE.serializeCases.length,
      15,
      '序列化样本必须恰好 15 条：条数变了说明夹具动了，两侧读取器都得跟着看',
    );
    assert.equal(
      FIXTURE.scanCases.length,
      11,
      '名字扫描样本必须恰好 11 条：条数变了说明夹具动了，两侧读取器都得跟着看',
    );
  });

  for (const testCase of FIXTURE.mergeCases) {
    it(testCase.name, () => {
      const pool = mergePool(testCase.env, testCase.disabled, testCase.dynamic);
      assert.deepEqual(pool, testCase.expected);
    });
  }

  for (const testCase of FIXTURE.serializeCases) {
    it(testCase.name, () => {
      assert.equal(serializeValue(testCase.value, testCase.mode), testCase.expected);
    });
  }

  for (const testCase of FIXTURE.scanCases) {
    it(testCase.name, () => {
      const names = listDynamicNames(testCase.parts);
      assert.deepEqual([...names].sort(), [...testCase.expected].sort());
    });
  }
});

describe('样本本身钉住的几条结论', () => {
  it('text 与 auto 对同一个数组的输出完全一致', () => {
    const auto = FIXTURE.serializeCases.find(
      (c) => c.mode === 'auto' && c.expected.startsWith('['),
    );
    const text = FIXTURE.serializeCases.find(
      (c) => c.mode === 'text' && c.expected.startsWith('['),
    );
    assert.ok(auto && text, '样本里缺 text / auto 的数组用例');
    assert.equal(auto.expected, text.expected);
  });

  it('json 给标量加引号，auto 与 text 不加', () => {
    const json = FIXTURE.serializeCases.find((c) => c.mode === 'json' && c.value === 'A001');
    const auto = FIXTURE.serializeCases.find((c) => c.mode === 'auto' && c.value === 'A001');
    assert.ok(json && auto, '样本里缺字符串用例');
    assert.equal(json.expected, '"A001"');
    assert.equal(auto.expected, 'A001');
  });
});
