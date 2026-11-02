/**
 * OAuth2 token 注入的共享样本读取器。
 *
 * 判据来自共享样本 `fixtures/oauth2-parity.json`，Rust 那一侧
 * （`crates/simple-post-core/src/oauth2/inject_parity_test.rs`）读同一份文件。两份实现各写
 * 一遍规则，只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了一边就没人提醒
 * 另一边。
 *
 * 样本比的是**结果**（注入后的 URL 与请求头列表），不是写法——Rust 侧就地改，渲染进程侧
 * 返回新对象，形状本来就不同。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import type { AuthConfig, OAuth2TokenResult } from '@/types/domain';

import { applyOAuth2Token, DEFAULT_OAUTH2 } from '../oauth2';
import type { PreparedRequest } from '../http-build';

interface ParityCase {
  name: string;
  tokenLocation: 'header' | 'query';
  tokenParam: string;
  token: { accessToken: string; tokenType: string };
  headers: Array<[string, string]>;
  url: string;
  expected: { url: string; headers: Array<[string, string]> };
}

const FIXTURE: { cases: ParityCase[] } = JSON.parse(
  readFileSync(new URL('../../../fixtures/oauth2-parity.json', import.meta.url), 'utf8'),
);

/** 一份已解析好的请求。注入只动 URL 与请求头，其余字段原样带着。 */
function prepared(c: ParityCase): PreparedRequest {
  return {
    method: 'GET',
    url: c.url,
    headers: c.headers.map(([k, v]) => [k, v] as [string, string]),
    body: null,
    bodyBase64: null,
    missingVars: [],
    missingPathParams: [],
  };
}

function auth(c: ParityCase): AuthConfig {
  return {
    kind: 'oauth2',
    oauth2: { ...DEFAULT_OAUTH2, tokenLocation: c.tokenLocation, tokenParam: c.tokenParam },
  };
}

function token(c: ParityCase): OAuth2TokenResult {
  return {
    accessToken: c.token.accessToken,
    tokenType: c.token.tokenType,
    expiresAt: null,
    scope: null,
    refreshed: false,
  };
}

describe('OAuth2 token 的注入与 Rust 侧一致', () => {
  it('样本里至少有一个 case', () => {
    assert.ok(FIXTURE.cases.length > 0, '样本里必须有 case，否则这条测试是空的');
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      const out = applyOAuth2Token(prepared(c), auth(c), token(c));
      assert.equal(out.url, c.expected.url, '注入后的 URL');
      assert.deepEqual(out.headers, c.expected.headers, '注入后的请求头');
    });
  }
});

/**
 * 样本本身得覆盖得住注入规则的每一条。
 */
describe('样本覆盖住了注入规则的每一条', () => {
  const names = FIXTURE.cases.map((c) => c.name);

  const needed = [
    'header 档补一条 Authorization',
    'header 档的 token 类型是空串时只放裸 token',
    'header 档去掉 token 类型首尾空白后透传，不改成 Bearer',
    'header 档的 token 类型带首尾空白时先去掉空白',
    'header 档不覆盖用户自己写的 Authorization',
    'header 档认大小写不敏感的 authorization',
    'query 档把 token 追加到已有的查询串后面',
    'query 档给没有查询串的地址起一个查询串',
    'query 档的参数名是空白时回落成 access_token',
    'query 档用配置的那个参数名',
    'query 档的参数名带首尾空白时按原值当参数名',
    'query 档的 token 值按查询串的字符集编码',
    'query 档的参数名也按查询串的字符集编码',
  ];

  for (const one of needed) {
    it(`样本里有「${one}」这条规则`, () => {
      assert.ok(names.includes(one), '样本里缺了这条规则，两侧就可能在这里分叉');
    });
  }

  it('样本里没有多出未登记的规则', () => {
    // 反向也查一遍：新增一条 case 却忘了登记进上面那份名单时，样本与名单就会各说各话，
    // 而这条用例仍然全绿。
    assert.equal(
      names.length,
      needed.length,
      `样本里有未登记的 case：${names.filter((n) => !needed.includes(n)).join('、')}`,
    );
  });
});
