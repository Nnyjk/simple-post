/**
 * The add control must be the same control whether the list is empty or
 * not.
 *
 * It used to be two. `KeyValueTable` rendered a centred solid
 * `variant="default"` button inside the empty state and a hairline-divided
 * text button at the foot of a populated list, so the two `添加服务` /
 * `添加变量` buttons sitting a few centimetres apart in the environment
 * panel had different weight, padding and colour. Whichever state you
 * happened to be in decided which control you were looking at.
 *
 * The fix hoists the control into a single element shared by both branches.
 * These tests assert that parity at the markup level rather than by
 * re-stating the class list, so the guard keeps holding if the styling is
 * retuned — only a real second control can break it.
 *
 * Rendering goes through `react-dom/server`; the repo has no DOM harness
 * (no Vitest / RTL — see AGENTS.md) and nothing here depends on effects.
 *
 * Run via:
 *   npx tsx --test src/features/settings/sections/rows/__tests__/key-value-table.test.ts
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { KeyValueTable } from '../KeyValueTable';

const ADD_TEXT = '添加变量';
const EMPTY_TEXT = '暂无变量';

/**
 * `KeyValueTable` is generic in its row type, and routing it through
 * `createElement` erases that generic — it lands on `unknown` and every
 * typed prop then fails to assign. Calling it directly keeps `T` pinned at
 * the call site.
 *
 * Safe here specifically because `KeyValueTable` uses no hooks; it is a
 * pure function of its props that hands `renderRow`'s output straight back
 * to the renderer. If it ever grows a hook, this has to become real
 * mounting rather than a direct call.
 */
function renderTable(items: string[]): string {
  return renderToStaticMarkup(
    KeyValueTable<string>({
      items,
      keyOf: (item) => item,
      emptyText: EMPTY_TEXT,
      addText: ADD_TEXT,
      onAdd: () => {},
      renderRow: (item) => createElement('span', null, item),
    }),
  );
}

/** The markup of the control labelled `addText`, for a direct comparison. */
function addControl(html: string): string {
  const re = new RegExp(
    `<button[^>]*>(?:(?!</button>)[\\s\\S])*${ADD_TEXT}(?:(?!</button>)[\\s\\S])*</button>`,
  );
  const match = html.match(re);
  assert.ok(match, `expected an add control labelled 「${ADD_TEXT}」`);
  return match![0];
}

const emptyHtml = renderTable([]);
const filledHtml = renderTable(['token', 'baseUrl']);

describe('KeyValueTable — add control parity', () => {
  it('renders byte-identical markup for the add control in both states', () => {
    assert.equal(
      addControl(emptyHtml),
      addControl(filledHtml),
      'the empty state must reuse the populated state’s add control, not a variant of it',
    );
  });

  it('keeps the empty prompt, and only the empty state shows it', () => {
    assert.match(emptyHtml, new RegExp(EMPTY_TEXT));
    assert.doesNotMatch(filledHtml, new RegExp(EMPTY_TEXT));
  });

  it('places the control after the hairline divider in both states', () => {
    // The divider is what makes it read as "the foot of the list" rather
    // than a floating action, so it has to be there when empty as well.
    for (const html of [emptyHtml, filledHtml]) {
      assert.match(addControl(html), /border-t border-border\/60/);
    }
  });

  it('does not fall back to a filled primary button when empty', () => {
    // The drift's signature: `variant="default"` renders the primary
    // background. Neither state should carry it now.
    assert.doesNotMatch(emptyHtml, /bg-primary/);
  });

  it('still renders its rows and the control when populated', () => {
    // Guards the refactor's other half: hoisting the control must not have
    // dropped it from the populated branch.
    assert.match(filledHtml, /token/);
    assert.match(filledHtml, /baseUrl/);
    assert.match(filledHtml, new RegExp(ADD_TEXT));
  });
});
