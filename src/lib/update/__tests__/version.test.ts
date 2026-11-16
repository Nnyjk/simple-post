/**
 * 「有没有新版」的比较规则。它是设置卡片与顶栏入口的唯一判据，故逐条钉住：
 * 前导 `v`、预发布后缀、段数不齐、以及认不出的版本号。
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { compareVersions, shouldPromptUpdate } from '../version';

describe('compareVersions', () => {
  it('按段比较，缺段按 0', () => {
    assert.equal(compareVersions('0.1.35', '0.1.34'), 1);
    assert.equal(compareVersions('0.1.34', '0.1.35'), -1);
    assert.equal(compareVersions('0.2', '0.2.0'), 0);
    assert.equal(compareVersions('1.0.0', '0.99.99'), 1);
  });

  it('数字段按数值比，不按字典序', () => {
    assert.equal(compareVersions('0.1.10', '0.1.9'), 1);
    assert.equal(compareVersions('0.10.0', '0.9.0'), 1);
  });

  it('前导 v 与预发布后缀不影响比较', () => {
    assert.equal(compareVersions('v0.1.35', '0.1.35'), 0);
    assert.equal(compareVersions('0.1.35-beta.1', '0.1.34'), 1);
  });

  it('空白或认不出的版本号返回 null', () => {
    assert.equal(compareVersions('', '0.1.35'), null);
    assert.equal(compareVersions('0.1.35', '  '), null);
    assert.equal(compareVersions('abc', '0.1.35'), null);
    assert.equal(compareVersions('0.1.x', '0.1.35'), null);
    assert.equal(compareVersions('0.1.35', '0.1.35.4'), -1);
  });
});

describe('shouldPromptUpdate', () => {
  it('插件/服务端说没有新版时一律不提示', () => {
    assert.equal(shouldPromptUpdate(false, '0.1.99', '0.1.34'), false);
  });

  it('latest 不高于当前版本不提示', () => {
    assert.equal(shouldPromptUpdate(true, '0.1.34', '0.1.34'), false);
    assert.equal(shouldPromptUpdate(true, '0.1.33', '0.1.34'), false);
  });

  it('latest 高于当前版本才提示', () => {
    assert.equal(shouldPromptUpdate(true, '0.1.35', '0.1.34'), true);
  });

  it('还没钉住当前版本时信发布的结论', () => {
    assert.equal(shouldPromptUpdate(true, '0.1.35', ''), true);
  });

  it('版本号认不出时信发布的结论，不吞掉提示', () => {
    assert.equal(shouldPromptUpdate(true, 'nightly', '0.1.34'), true);
  });
});
