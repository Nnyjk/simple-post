/**
 * `toRequestSnapshot` —— 一次发送出去的请求折成可落库的快照。
 *
 * 保护的是两件事：
 *   - 快照里的 header 是**线上那一份**（后端报上来的，含传输层补的那几条），不是界面上
 *     拼出来的那几条；
 *   - 后端报不上来时（请求从未抵达：超时、网络错误、中止）回落到本地那份，而不是把
 *     header 丢空。
 *
 * 运行：npx tsx --test src/features/http-tester/__tests__/request-snapshot.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { toRequestSnapshot } from '../model/requestInput';
import type { PreparedRequest } from '@/lib/http-build';

const PREPARED: PreparedRequest = {
  method: 'POST',
  url: 'http://127.0.0.1:8899/api/orders?page=1',
  headers: [
    ['x-token', 'demo-token-001'],
    ['content-type', 'application/json'],
  ],
  body: '{"orderId":"42"}',
} as PreparedRequest;

describe('toRequestSnapshot', () => {
  it('有线上那一份请求头时用它，而不是本地拼的那几条', () => {
    const snapshot = toRequestSnapshot(PREPARED, [
      ['x-token', 'demo-token-001'],
      ['content-type', 'application/json'],
      ['accept', '*/*'],
      ['accept-encoding', 'gzip,deflate,br'],
      ['host', '127.0.0.1:8899'],
      ['content-length', '18'],
    ]);

    assert.deepEqual(
      snapshot.headers.map(([name]) => name),
      ['x-token', 'content-type', 'accept', 'accept-encoding', 'host', 'content-length'],
      '传输层补的那几条必须在快照里',
    );
    assert.equal(snapshot.method, 'POST');
    assert.equal(snapshot.url, PREPARED.url);
    assert.equal(snapshot.body, PREPARED.body);
  });

  it('线上那一份缺席时回落到本地拼的 header', () => {
    const snapshot = toRequestSnapshot(PREPARED);
    assert.deepEqual(snapshot.headers, [
      ['x-token', 'demo-token-001'],
      ['content-type', 'application/json'],
    ]);
  });

  it('快照不与传入的那份共享 header 数组', () => {
    const snapshot = toRequestSnapshot(PREPARED);
    snapshot.headers.push(['x-extra', '1']);
    assert.equal(PREPARED.headers.length, 2, '快照改了不该动到调用方手里的那份');
  });
});
