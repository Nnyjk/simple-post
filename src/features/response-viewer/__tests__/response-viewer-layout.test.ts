/**
 * 响应查看器布局的源码级回归网。
 *
 * 为什么只能读源码：本 runner（`tsx --test` + `node:assert`）没有 DOM、没有 jsdom，
 * 也没有 React 渲染器，测不了 `ResponseViewer` 的任何实际尺寸。
 *
 * 它钉住的是当初真正坏掉的那件事：包住历史条的那个元素的形状。`<WorkspaceShell>` 把
 * 查看器渲染在 `min-h-0 flex-1` 里，而那不是 flex 容器，故 `ResponseViewer` 的每个分支
 * 都必须自带一个 flex 列 —— 否则带 `h-full` 的兄弟会吃光整个盒子，其后按块流排布的
 * 内容就落到屏幕外。
 *
 * 读源码是钝办法，但这是本仓库对「功能测试看不见的那类缺陷」的既定取舍，见
 * `crates/simple-post-mcp/src/mcp/mod_test.rs` 里的 `include_str!` 回归网，理由相同：
 * JSON 信封形状错了照样能反序列化，运行时不会有任何东西失败。
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const SRC = readFileSync(fileURLToPath(new URL('../ResponseViewer.tsx', import.meta.url)), 'utf8');

/** 剥掉 `//` 注释，免得文件自己的散文满足某个模式。 */
function code(source: string): string {
  return source.replace(/^\s*\/\/.*$/gm, '');
}

/**
 * 这行的 `className` 是否给出了一个 flex 容器？
 * 不是 `display:flex` 的包装会按块流排布子元素，正是这里要防的失效模式。
 */
function isFlexColumn(className: string): boolean {
  const has = (token: string) => new RegExp(`(?:^|\\s)${token}(?:\\s|$)`).test(className);
  return has('flex') && has('flex-col');
}

/**
 * `if (!response) { … }` 这一个分支的文本，两侧都设了界。
 *
 * 右侧的界很关键：若只从 `if (!response)` 切到文件末尾，「空状态分支」的匹配会一路
 * 向后抓到*有数据*分支的 flex 容器，测试就会拿它本该拒绝的那段片段通过 —— 一条不可能
 * 失败的回归测试比没有更糟，因为它随后会被当作证据引用。
 *
 * `const statusColor` 是有数据分支的第一条语句，故它之前的都属于空状态分支。
 */
function emptyBranch(): string {
  const body = code(SRC);
  const start = body.indexOf('if (!response)');
  assert.notEqual(start, -1, 'the empty-state branch is gone');
  const end = body.indexOf('const statusColor', start);
  assert.notEqual(end, -1, 'the populated branch marker is gone; update this slice');
  return body.slice(start, end);
}

describe('ResponseViewer: both branches give the history strip a flex column to sit in', () => {
  it('renders the strip in the populated branch too', () => {
    // 少了这条，下面的用例在一个只渲染了一个分支的查看器上也会通过。
    const occurrences = code(SRC).match(/<ResponseHistory\s*\/>/g) ?? [];
    assert.equal(
      occurrences.length,
      2,
      'expected the strip in both the empty and the populated branch',
    );
  });

  it('the empty-state branch wraps its children in a flex column', () => {
    const branch = emptyBranch();
    const wrapper = branch.match(/return\s*\(\s*<section\s+className="([^"]+)"/);
    assert.ok(
      wrapper,
      'the empty branch must return a single <section>, not a bare fragment: ' +
        "a fragment under WorkspaceShell's non-flex wrapper puts the strip " +
        'below the box instead of inside it',
    );
    assert.ok(
      isFlexColumn(wrapper[1]),
      `the empty branch's wrapper must be a flex column, got: ${wrapper[1]}`,
    );
  });

  it('the populated branch wraps its children in a flex column', () => {
    const body = code(SRC);
    const populated = body.slice(body.indexOf('const statusColor'));
    const wrapper = populated.match(/<section\s+className="([^"]*h-full[^"]*)"/);
    assert.ok(wrapper, "expected the populated branch's <section>");
    assert.ok(
      isFlexColumn(wrapper[1]),
      `the populated branch's wrapper must be a flex column, got: ${wrapper[1]}`,
    );
  });

  it('the empty state flexes rather than claiming the whole box', () => {
    // 空状态上的 `h-full` 是光靠包装修不掉的另一半：父级是 flex 列时它仍会解算成父级的
    // 完整高度，把历史条挤出去。`flex-1` 取的是历史条落位后真正剩下的空间。
    const emptyState = code(SRC.slice(code(SRC).indexOf('function EmptyState')));
    const classes = emptyState.match(/<section\s+className="([^"]+)"/)?.[1] ?? '';
    assert.ok(classes, 'could not read the EmptyState section');
    assert.ok(/(?:\s)flex-1(?:\s|$)/.test(classes), `EmptyState must use flex-1, got: ${classes}`);
    assert.ok(
      !/(?:\s)h-full(?:\s|$)/.test(classes),
      `EmptyState must not use h-full beside a sibling strip, got: ${classes}`,
    );
  });
});

describe('the strip it is protecting lives inside that column', () => {
  it('ResponseHistory itself is shrink-0, not flex-1', () => {
    // 与上面那条互为对照：历史条若也是 `flex-1`，就会与空状态争抢盒子，而不是在底部
    // 取自己的自然高度。
    const strip = readFileSync(
      fileURLToPath(new URL('../ResponseHistory.tsx', import.meta.url)),
      'utf8',
    );
    assert.ok(
      /shrink-0/.test(strip),
      'the strip must keep its height with shrink-0, or the empty state ' + 'cannot grow above it',
    );
  });
});
