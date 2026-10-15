/**
 * `http-build.ts` 的查询串与 header 组装单测（`buildFetchInit` 适配器形状）。
 *
 * 覆盖：查询串的 `?`/`&` 选择与编码、用户 header 的顺序、自动补的
 * Content-Type / Accept，以及与用户 header 的去重。认证、模板、body 与 cURL
 * 一致性在同目录的其它分片里。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildFetchInit } from '../http-build';
import { kv, input } from './http-build-fixtures';
// ============================================================
// A. 查询串拼装
// ============================================================

describe('http-build: query string', () => {
  it('leaves URL unchanged when params is empty', () => {
    const { url } = buildFetchInit(input({ params: [] }));
    assert.equal(url, 'http://api.example.com/x');
  });

  it('appends a single param with ?', () => {
    const { url } = buildFetchInit(input({ params: [kv('page', '1')] }));
    assert.equal(url, 'http://api.example.com/x?page=1');
  });

  it('joins multiple params with &', () => {
    const { url } = buildFetchInit(
      input({ params: [kv('page', '1'), kv('size', '20'), kv('sort', 'desc')] }),
    );
    assert.equal(url, 'http://api.example.com/x?page=1&size=20&sort=desc');
  });

  it('uses & when URL already has ?', () => {
    const { url } = buildFetchInit(
      input({ url: 'http://api.example.com/x?foo=bar', params: [kv('page', '1')] }),
    );
    assert.equal(url, 'http://api.example.com/x?foo=bar&page=1');
  });

  it('uses & when URL already has multiple query params', () => {
    const { url } = buildFetchInit(
      input({ url: 'http://api.example.com/x?foo=bar&baz=qux', params: [kv('page', '1')] }),
    );
    assert.equal(url, 'http://api.example.com/x?foo=bar&baz=qux&page=1');
  });

  it('skips disabled params', () => {
    const { url } = buildFetchInit(input({ params: [kv('a', '1'), kv('b', '2', false)] }));
    assert.equal(url, 'http://api.example.com/x?a=1');
  });

  it('skips params with empty key', () => {
    const { url } = buildFetchInit(input({ params: [kv('', 'orphan'), kv('kept', 'yes')] }));
    assert.equal(url, 'http://api.example.com/x?kept=yes');
  });

  it('skips params with empty value', () => {
    const { url } = buildFetchInit(input({ params: [kv('a', ''), kv('b', '2')] }));
    assert.equal(url, 'http://api.example.com/x?b=2');
  });

  it('URL-encodes special characters in key and value', () => {
    const { url } = buildFetchInit(input({ params: [kv('a b', 'x&y=z+1')] }));
    // encodeURIComponent: ' ' -> '%20', '&' -> '%26', '=' -> '%3D', '+' -> '%2B'
    assert.equal(url, 'http://api.example.com/x?a%20b=x%26y%3Dz%2B1');
  });

  it('URL-encodes unicode in value', () => {
    const { url } = buildFetchInit(input({ params: [kv('q', '中文')] }));
    // '中' -> %E4%B8%AD, '文' -> %E6%96%87
    assert.equal(url, 'http://api.example.com/x?q=%E4%B8%AD%E6%96%87');
  });
});

// ============================================================
// B. header 顺序 / 自动 Content-Type / 自动 Accept
// ============================================================

describe('http-build: headers', () => {
  it('emits a user header alongside the auto-injected Accept', () => {
    const { init } = buildFetchInit(input({ headers: [kv('X-Trace', 'abc')] }));
    assert.deepEqual(init.headers, {
      'X-Trace': 'abc',
      Accept: 'application/json',
    });
  });

  it('skips disabled headers', () => {
    const { init } = buildFetchInit(input({ headers: [kv('A', '1', false), kv('B', '2')] }));
    assert.deepEqual(init.headers, { B: '2', Accept: 'application/json' });
  });

  it('skips headers with empty key', () => {
    const { init } = buildFetchInit(input({ headers: [kv('', 'orphan'), kv('B', '2')] }));
    assert.deepEqual(init.headers, { B: '2', Accept: 'application/json' });
  });

  it('preserves the user-set Content-Type (case-insensitive)', () => {
    const { init } = buildFetchInit(
      input({
        body: { mode: 'json', content: '{"a":1}' },
        headers: [kv('content-type', 'application/vnd.custom+json')],
      }),
    );
    // 用户值优先；自动补的 application/json 不得覆盖它。
    assert.equal(
      (init.headers as Record<string, string>)['content-type'],
      'application/vnd.custom+json',
    );
    assert.equal((init.headers as Record<string, string>)['Content-Type'], undefined);
  });

  it('auto-adds Content-Type for json mode when user has none', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'json', content: '{}' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/json');
  });

  it('auto-adds Content-Type for form mode', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'form', content: 'a=1' } }));
    assert.equal(
      (init.headers as Record<string, string>)['Content-Type'],
      'application/x-www-form-urlencoded',
    );
  });

  it('auto-adds Content-Type for raw mode', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'raw', content: 'hello' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'text/plain');
  });

  it('does NOT auto-add Content-Type when body.mode is none', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'none', content: '' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], undefined);
  });

  it('auto-adds Content-Type even when json content is empty (mode implies a body shape)', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'json', content: '' } }));
    assert.equal((init.headers as Record<string, string>)['Content-Type'], 'application/json');
  });

  it('auto-adds Accept: application/json when user has none', () => {
    const { init } = buildFetchInit(input());
    assert.equal((init.headers as Record<string, string>)['Accept'], 'application/json');
  });

  it('respects user-set Accept (case-insensitive)', () => {
    const { init } = buildFetchInit(input({ headers: [kv('accept', 'text/html')] }));
    assert.equal((init.headers as Record<string, string>)['accept'], 'text/html');
    assert.equal((init.headers as Record<string, string>)['Accept'], undefined);
  });

  it('keeps multiple user headers in declared order', () => {
    const { init } = buildFetchInit(
      input({
        body: { mode: 'json', content: '{}' },
        headers: [kv('X-Trace', '1'), kv('X-Other', '2')],
      }),
    );
    const keys = Object.keys(init.headers as Record<string, string>);
    // 确切插入顺序取决于实现里 header 数组收成 record 的方式，但这两个用户 header
    // 必须排在任何自动补的（Content-Type、Accept）之前
    assert.ok(keys.indexOf('X-Trace') < keys.indexOf('Content-Type'));
    assert.ok(keys.indexOf('X-Other') < keys.indexOf('Accept'));
  });
});
