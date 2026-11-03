/**
 * OAuth2 配置的 `{{var}}` 替换规则的共享样本读取器。
 *
 * 判据来自共享样本 `fixtures/oauth2-config-parity.json`，Rust 那一侧
 * （`crates/simple-post-core/src/oauth2/config_parity_test.rs`）读同一份文件。
 *
 * 替换规则只有 Rust 一份实现（`simple_post_core::oauth2::substitute_config`），渲染进程
 * 不自己替换 —— 它只需要知道同一份字段清单（`OAUTH2_SUBSTITUTED_FIELDS`），好把没解析
 * 出来的变量报出来。故本读取器验的是三件事：
 *
 *   1. 渲染进程那份字段清单与样本声明的清单逐条相同；
 *   2. 渲染进程**不**替换 OAuth2 配置（替换由后端在取 token 之前做）；
 *   3. 渲染进程报出的缺变量清单与样本相同 —— 密钥与一次性输入里的变量不算缺。
 *
 * 第 4 件事（替换后的配置长什么样）只有 Rust 侧能验：渲染进程不做替换。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import type { AuthConfig, OAuth2Auth } from '@/types/domain';

import { OAUTH2_SUBSTITUTED_FIELDS, resolveAuth } from '../http-build/auth';
import { createMissingTracker } from '../http-build/pool';
import { DEFAULT_OAUTH2 } from '../oauth2';

interface ParityCase {
  name: string;
  vars: Record<string, string>;
  config: Partial<OAuth2Auth>;
  expected: Partial<OAuth2Auth>;
  missing: string[];
}

interface Fixture {
  substitutedFields: string[];
  keptFields: string[];
  cases: ParityCase[];
}

const FIXTURE: Fixture = JSON.parse(
  readFileSync(new URL('../../../fixtures/oauth2-config-parity.json', import.meta.url), 'utf8'),
);

/** 样本里出现的键必须都是已知的：键名漂了会让整条测试静默空转。 */
function assertKnownKeys(value: unknown, allowed: readonly string[], where: string): void {
  assert.ok(value && typeof value === 'object' && !Array.isArray(value), `${where} 必须是对象`);
  for (const key of Object.keys(value as Record<string, unknown>)) {
    assert.ok(allowed.includes(key), `${where} 里有不认识的键：${key}`);
  }
}

/** 一份带样本配置的 OAuth2 鉴权配置。 */
function authOf(c: ParityCase): AuthConfig {
  return { kind: 'oauth2', oauth2: { ...DEFAULT_OAUTH2, ...c.config } };
}

describe('OAuth2 配置的变量替换规则与 Rust 侧一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  it('样本的键名都是已知的', () => {
    assertKnownKeys(FIXTURE, ['substitutedFields', 'keptFields', 'cases'], '样本');
    const configKeys = Object.keys(DEFAULT_OAUTH2);
    for (const c of FIXTURE.cases) {
      assertKnownKeys(c, ['name', 'vars', 'config', 'expected', 'missing'], `case「${c.name}」`);
      assertKnownKeys(c.config, configKeys, `case「${c.name}」的 config`);
      assertKnownKeys(c.expected, configKeys, `case「${c.name}」的 expected`);
    }
  });

  it('被替换的字段清单与样本一致', () => {
    assert.deepEqual([...OAUTH2_SUBSTITUTED_FIELDS], FIXTURE.substitutedFields);
  });

  it('刻意不替换的字段与被替换的字段没有交集', () => {
    for (const field of FIXTURE.keptFields) {
      assert.ok(!FIXTURE.substitutedFields.includes(field), `${field} 同时出现在两份清单里`);
    }
  });

  for (const c of FIXTURE.cases) {
    it(`渲染进程不替换 OAuth2 配置：${c.name}`, () => {
      const tracker = createMissingTracker();
      const auth = authOf(c);
      const out = resolveAuth(auth, c.vars, tracker);

      // 替换由后端做：渲染进程交出去的配置与进来时逐字相同。
      for (const field of OAUTH2_SUBSTITUTED_FIELDS) {
        assert.equal(out.oauth2?.[field], auth.oauth2?.[field], `${field} 不该在渲染进程被替换`);
      }
      // 缺变量清单与样本相同。
      assert.deepEqual(tracker.list(), c.missing, '缺变量清单');
    });

    it(`刻意不替换的字段原样留着：${c.name}`, () => {
      const config = c.config as Record<string, unknown>;
      const expected = c.expected as Record<string, unknown>;
      for (const field of FIXTURE.keptFields) {
        assert.equal(expected[field], config[field], `${field} 不该被替换`);
      }
    });
  }
});
