/**
 * 检查结果的中文文案。四类结果的优先级是「失败 → 有新版本 → 已是最新」：一次限流回来
 * 时不能同时说「已是最新」，那会让用户以为查过了。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { describeCheckError, describeCheckOutcome } from '../messages';

describe('describeCheckError', () => {
  it('没有错误码 → null', () => {
    assert.equal(describeCheckError(null), null);
    assert.equal(describeCheckError(undefined), null);
  });

  it('四个契约里的错误码各有中文说法', () => {
    assert.match(describeCheckError('unreachable') ?? '', /连不上/);
    assert.match(describeCheckError('rate_limited') ?? '', /限流/);
    assert.match(describeCheckError('no_release') ?? '', /还没有发布版本/);
    assert.match(describeCheckError('bad_response') ?? '', /读不懂/);
  });

  it('没见过的错误码带上原码，界面不留空', () => {
    assert.equal(describeCheckError('teapot'), '检查失败（teapot）');
  });
});

describe('describeCheckOutcome', () => {
  it('失败优先于版本结论', () => {
    assert.match(
      describeCheckOutcome({ available: false, latest: null, error: 'rate_limited' }),
      /限流/,
    );
  });

  it('有新版本时带上目标版本号', () => {
    assert.equal(
      describeCheckOutcome({ available: true, latest: '0.1.35', error: null }),
      '发现新版本 v0.1.35',
    );
  });

  it('没有新版本 → 已是最新', () => {
    assert.equal(
      describeCheckOutcome({ available: false, latest: null, error: null }),
      '已是最新版本',
    );
  });
});
