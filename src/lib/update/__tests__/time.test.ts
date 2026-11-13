/**
 * 更新检查的时间规则：节流判定与相对时间文案。
 *
 * 「现在」由调用方给，故这些用例不依赖真实时钟；每条都钉住一个具体的边界（刚好 24 小时、
 * 未来时间戳、解析不出来的字符串），因为这几处正是界面会默默出错的地方。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { dueForCheck, formatDate, relativeTime, UPDATE_CHECK_INTERVAL_MS } from '../time';

const NOW = Date.parse('2026-11-13T12:00:00.000Z');
const iso = (msAgo: number) => new Date(NOW - msAgo).toISOString();

describe('dueForCheck', () => {
  it('从未检查过 → 该查', () => {
    assert.equal(dueForCheck(null, NOW), true);
    assert.equal(dueForCheck('', NOW), true);
    assert.equal(dueForCheck(undefined, NOW), true);
  });

  it('刚好满 24 小时 → 该查', () => {
    assert.equal(dueForCheck(iso(UPDATE_CHECK_INTERVAL_MS), NOW), true);
  });

  it('差一分钟不满窗口 → 不查', () => {
    assert.equal(dueForCheck(iso(UPDATE_CHECK_INTERVAL_MS - 60_000), NOW), false);
  });

  it('上次检查落在未来（时钟被改过）→ 该查，而不是一直等到那个未来时刻', () => {
    assert.equal(dueForCheck(iso(-60_000), NOW), true);
  });

  it('解析不出来的时间戳 → 该查', () => {
    assert.equal(dueForCheck('not-a-date', NOW), true);
  });

  it('窗口可由调用方覆盖', () => {
    assert.equal(dueForCheck(iso(2_000), NOW, 1_000), true);
    assert.equal(dueForCheck(iso(500), NOW, 1_000), false);
  });
});

describe('relativeTime', () => {
  it('没有时间戳 → null，由调用方决定文案', () => {
    assert.equal(relativeTime(null, NOW), null);
    assert.equal(relativeTime('', NOW), null);
    assert.equal(relativeTime('nonsense', NOW), null);
  });

  it('一分钟以内说「刚刚」', () => {
    assert.equal(relativeTime(iso(0), NOW), '刚刚');
    assert.equal(relativeTime(iso(59_000), NOW), '刚刚');
  });

  it('分钟 / 小时 / 天 / 月 / 年各取整', () => {
    assert.equal(relativeTime(iso(5 * 60_000), NOW), '5 分钟前');
    assert.equal(relativeTime(iso(3 * 3_600_000), NOW), '3 小时前');
    assert.equal(relativeTime(iso(2 * 86_400_000), NOW), '2 天前');
    assert.equal(relativeTime(iso(90 * 86_400_000), NOW), '3 个月前');
    assert.equal(relativeTime(iso(400 * 86_400_000), NOW), '1 年前');
  });

  it('未来时间戳说「刚刚」，不出现负数', () => {
    assert.equal(relativeTime(iso(-3 * 3_600_000), NOW), '刚刚');
  });
});

describe('formatDate', () => {
  it('写出本地日历日期', () => {
    const local = new Date(2026, 10, 13, 9, 30);
    assert.equal(formatDate(local.toISOString()), '2026-11-13');
  });

  it('没有或读不出时间 → null', () => {
    assert.equal(formatDate(null), null);
    assert.equal(formatDate(''), null);
    assert.equal(formatDate('2026-13-99'), null);
  });
});
