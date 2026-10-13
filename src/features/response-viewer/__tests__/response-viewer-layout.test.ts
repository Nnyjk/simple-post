/**
 * Source-level regression net for the response viewer's layout.
 *
 * Why source-level: this runner (`tsx --test` + `node:assert`) has no DOM,
 * no jsdom, and no React renderer — a test cannot mount `ResponseViewer`
 * and measure anything. And a *behavioural* test could not have caught the
 * original defect anyway, because the defect had no behaviour: the
 * history strip was mounted, rendered in full, and sitting correctly
 * inside its own box — just below the bottom edge of a container that
 * clipped it. A DOM query for `response-history-toggle` found it and
 * reported "历史 1" while the user was looking at a panel with no strip in
 * it at all.
 *
 * So this pins the thing that actually broke: the shape of the element
 * that wraps the strip. `<WorkspaceShell>` renders the viewer inside
 * `min-h-0 flex-1`, which is not a flex container, so each branch of
 * `ResponseViewer` has to supply its own flex column — otherwise a
 * sibling with `h-full` consumes the whole box and whatever follows it in
 * block flow lands off-screen.
 *
 * It reads the source as text, which is blunt. That is the accepted
 * trade in this repo for classes of defect no functional test can see —
 * see the `include_str!` nets in `crates/simple-post-mcp/src/mcp/
 * mod_test.rs`, which exist for the same reason (a mis-shaped JSON
 * envelope still deserialises, so nothing fails at runtime).
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const SRC = readFileSync(fileURLToPath(new URL('../ResponseViewer.tsx', import.meta.url)), 'utf8');

/** Strip `//` comments so the file's own prose cannot satisfy a pattern. */
function code(source: string): string {
  return source.replace(/^\s*\/\/.*$/gm, '');
}

/**
 * Is `className` on this line naming a flex container?
 * A wrapper that is not `display:flex` lays its children out in block
 * flow, which is the whole failure mode here.
 */
function isFlexColumn(className: string): boolean {
  const has = (token: string) => new RegExp(`(?:^|\\s)${token}(?:\\s|$)`).test(className);
  return has('flex') && has('flex-col');
}

/**
 * The text of one `if (!response) { … }` branch, bounded on BOTH sides.
 *
 * The bound on the right matters: an earlier version sliced only from
 * `if (!response)` to the end of the file, so the "empty branch" match
 * happily reached forward and found the *populated* branch's flex
 * container. The test then passed against the exact fragment it was
 * written to reject — a regression test that cannot fail is worse than
 * none, because it is later cited as proof.
 *
 * `const statusColor` is the first statement of the populated branch, so
 * everything before it is the empty one.
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
    // Without this the tests below would pass on a viewer that dropped
    // the strip from one branch — the original bug was the strip being
    // present in both and still invisible in one.
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
    // `h-full` on the empty state is the half of the bug the wrapper alone
    // does not fix: with a flex-column parent it still resolves to the
    // parent's full height and pushes the strip out. `flex-1` takes the
    // space that is actually left after the strip has been placed.
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
    // The counterpart of the check above: if the strip were `flex-1` it
    // would compete with the empty state for the box instead of taking
    // its own natural height at the bottom.
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
