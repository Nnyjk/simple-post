/**
 * 设置面板里的「历史保留条数」。
 *
 * 这一栏写回一次就是裁一次库，裁掉的行不会回来。故它必须**失焦或回车才提交**：
 * 每次按键就提交的话，用户把 100 改成 50 时第一键送出的 5 已经把库裁到 5 条。
 *
 * 这个 runner（`tsx --test` + `node:assert`）没有 DOM、没有 jsdom，故交互那一半
 * 只能读源码确认「接线」；解析那一半是纯函数，直接测。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { parseHistoryKeepDraft } from '@/stores/slices/history-keep';

const source = readFileSync(
  resolve(process.cwd(), 'src/features/settings/sections/RequestSection.tsx'),
  'utf8',
);

describe('历史保留条数：草稿的解析', () => {
  it('一个正常的数字解析成条数', () => {
    assert.equal(parseHistoryKeepDraft('50'), 50);
    assert.equal(parseHistoryKeepDraft(' 50 '), 50);
    assert.equal(parseHistoryKeepDraft('1'), 1);
  });

  it('空串与空白解析不出来', () => {
    assert.equal(parseHistoryKeepDraft(''), null);
    assert.equal(parseHistoryKeepDraft('   '), null);
  });

  it('不是数字的解析不出来', () => {
    assert.equal(parseHistoryKeepDraft('abc'), null);
    assert.equal(parseHistoryKeepDraft('NaN'), null);
    assert.equal(parseHistoryKeepDraft('Infinity'), null);
  });

  it('小于 1 的解析不出来', () => {
    // 0 与负数都不是「保留多少条」，拿去裁库会把未收藏的行全删掉。
    assert.equal(parseHistoryKeepDraft('0'), null);
    assert.equal(parseHistoryKeepDraft('-3'), null);
  });
});

describe('历史保留条数：提交挂在失焦与回车上', () => {
  it('写回只有一处，且在提交函数里', () => {
    const calls = source.match(/updateResponseHistoryKeep\(/g) ?? [];
    assert.equal(
      calls.length,
      1,
      '写回只该在提交函数里发生一次；每次按键都写回会把库裁成用户刚敲的那个半截数',
    );
  });

  it('输入框的 onChange 只改草稿', () => {
    assert.ok(
      /onChange=\{\(e\) => setKeepDraft\(e\.target\.value\)\}/.test(source),
      '输入框每次按键只改草稿，不直接写回',
    );
  });

  it('草稿挂在失焦与回车上提交', () => {
    assert.ok(/onBlur=\{commitKeepDraft\}/.test(source), '失焦时提交草稿');
    assert.ok(
      /onKeyDown=\{\(e\) => \{[\s\S]{0,200}?e\.key === 'Enter'[\s\S]{0,200}?commitKeepDraft\(\)/.test(
        source,
      ),
      '回车时提交草稿',
    );
  });

  it('草稿解析不出来时退回当前值', () => {
    assert.ok(
      /parseHistoryKeepDraft\(keepDraft\)[\s\S]{0,200}?=== null[\s\S]{0,200}?setKeepDraft\(String\(historyKeep\)\)/.test(
        source,
      ),
      '坏草稿要退回当前值，不能拿它去裁库',
    );
  });
});
