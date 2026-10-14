/**
 * 无论列表为空与否，新增控件都必须是同一个控件。
 *
 * 做法是把控件提成一个由两个分支共用的元素。这些用例在标记层断言这种一致，而不是复述
 * 类名清单，故日后调整样式这条守卫仍然成立 —— 只有真的多出第二个控件才会破它。
 *
 * 渲染走 `react-dom/server`；本仓库没有 DOM 测试环境（没有 Vitest / RTL，见 AGENTS.md），
 * 且这里没有任何东西依赖 effect。
 *
 * 运行：
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
 * `KeyValueTable` 的行类型是泛型的，而经 `createElement` 走一遍会把那个泛型抹掉 ——
 * 它落到 `unknown`，此后每个带类型的 prop 都无法赋值。直接调用它能把 `T` 钉在调用点上。
 *
 * 之所以在这里这样做是安全的：`KeyValueTable` 不用任何 hook，它是 props 的纯函数，把
 * `renderRow` 的产出原样交回渲染器。它一旦长出 hook，这里就得改成真正的挂载而非直接调用。
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

/** 标签为 `addText` 的那个控件的标记，供直接比对。 */
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
    // 正是那条分隔线让它读作「列表末尾」而非一个浮空的动作，故为空时也得有。
    for (const html of [emptyHtml, filledHtml]) {
      assert.match(addControl(html), /border-t border-border\/60/);
    }
  });

  it('does not fall back to a filled primary button when empty', () => {
    // 漂移的特征：`variant="default"` 会渲染出主色底。现在两种状态都不该带它。
    assert.doesNotMatch(emptyHtml, /bg-primary/);
  });

  it('still renders its rows and the control when populated', () => {
    // 守住这次重构的另一半：把控件提上去不能顺手把它从有内容的分支里弄丢。
    assert.match(filledHtml, /token/);
    assert.match(filledHtml, /baseUrl/);
    assert.match(filledHtml, new RegExp(ADD_TEXT));
  });
});
