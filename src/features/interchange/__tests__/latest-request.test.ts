/**
 * 只认最新的一趟请求。
 *
 * 缺陷本身：换环境会重取一趟预览，但每一趟回来都无条件写进预览。连着换两个环境时，
 * 先发的那一趟可能后到——它带回来的 `servers` 属于**旧**环境，于是下拉显示着新
 * 环境，提示却按旧环境点名。这与本 PR 要修的是同一个毛病。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { createLatestRequest } from '../latest-request';

describe('只认最新的一趟请求', () => {
  it('后领的号才是最新的', () => {
    const guard = createLatestRequest();
    const first = guard.next();
    const second = guard.next();

    assert.equal(guard.isCurrent(second), true, '后领的那一趟是当前这一趟');
    assert.equal(guard.isCurrent(first), false, '先领的那一趟已经过期');
  });

  it('作废之后，先前领的号全部过期', () => {
    const guard = createLatestRequest();
    const ticket = guard.next();
    assert.equal(guard.isCurrent(ticket), true);

    // 弹窗关掉：这一趟回来时不该再往这个会话里写预览。
    guard.invalidate();

    assert.equal(guard.isCurrent(ticket), false, '作废之后那一趟是过期的');
    const next = guard.next();
    assert.equal(guard.isCurrent(next), true, '作废之后领的新号是当前的');
  });
});
