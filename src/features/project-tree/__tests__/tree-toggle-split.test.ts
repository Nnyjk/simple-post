/**
 * 「点箭头只展开、点行名只打开工作区」的接线回归网 —— 读源码，不渲染 React。
 *
 * **为什么只能读源码。** 本 runner（`tsx --test` + `node:assert`）没有 DOM、没有
 * jsdom、没有 React 渲染器，故测不了真实的鼠标点击。读源码是钝办法，但这是本仓库对
 * 「功能测试看不见的那类缺陷」的既定取舍，见 `tree-rename-selection.test.ts` 的开头，
 * 理由相同。
 *
 * **它钉住的是什么。** 树里的「打开工作区」与「展开 / 收起」曾经绑在同一个点击上：
 * 点行名既打开工作区又切换展开状态，点展开箭头也一样。两个后果都与用户当下的意图
 * 相反 —— 用户只想看内容却打开了工作区，用户只想收起分支却打开了工作区。
 *
 * 故两者必须各自独立：行按钮调 `openTab`，展开箭头调 `toggleNodeExpanded`，两个
 * 按钮是两个元素。
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

function readSource(relative: string): string {
  return readFileSync(fileURLToPath(new URL(relative, import.meta.url)), 'utf8');
}

/**
 * 剥掉注释，只留代码。
 *
 * 必须剥：注释里会提到「曾经两者绑在同一个点击上」这样的**反例**，不剥的话源码级断言
 * 会被自己的说明文字满足 —— 一条不可能失败的回归测试比没有更糟。
 */
function code(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
}

/**
 * 取出某个箭头函数的函数体。
 *
 * 结尾按「下一行是缩进两格的 `};`」来找，而不是按花括号配对 —— 函数体里没有嵌套的
 * 函数，按缩进找既够用又不会因为多一层括号而数错。
 */
function bodyOf(source: string, name: string): string {
  const start = source.indexOf(`const ${name} = `);
  assert.notEqual(start, -1, `源码里找不到 ${name}`);
  const end = source.indexOf('\n  };', start);
  assert.notEqual(end, -1, `源码里找不到 ${name} 的函数体结尾`);
  return source.slice(start, end);
}

/**
 * 取出包含 `marker` 的那个 JSX 元素：从它所在的 `<button` 到配对的 `</button>`。
 *
 * 「配对」在这里是「下一个 `</button>`」—— 若某个按钮里又套了按钮，这段文本就到内层
 * 那个为止，调用方据此能看出嵌套。
 */
function elementAround(source: string, marker: string): string {
  const at = source.indexOf(marker);
  assert.notEqual(at, -1, `源码里找不到 ${marker}`);
  const start = source.lastIndexOf('<button', at);
  const end = source.indexOf('</button>', at);
  assert.ok(start !== -1 && end !== -1, `源码里找不到 ${marker} 所在的按钮`);
  // 收尾标签一起取进来：配平检查要看到它。
  return source.slice(start, end + '</button>'.length);
}

const SELECTION = '../hooks/useTreeSelection.ts';
const SHELL = '../rows/TreeRow.tsx';

describe('打开工作区的那一半：不动展开状态', () => {
  it('打开模块工作区只开标签', () => {
    const body = bodyOf(code(readSource(SELECTION)), 'openModuleTab');
    assert.match(body, /openTab\('module', id\)/, '打开模块工作区没有开标签');
    assert.doesNotMatch(
      body,
      /toggleNodeExpanded/,
      '打开模块工作区仍在切换展开状态 —— 点行名会顺手收起刚展开的分支',
    );
  });

  it('打开集合工作区只开标签', () => {
    const body = bodyOf(code(readSource(SELECTION)), 'openCollectionTab');
    assert.match(body, /openTab\('collection', id\)/, '打开集合工作区没有开标签');
    assert.doesNotMatch(
      body,
      /toggleNodeExpanded/,
      '打开集合工作区仍在切换展开状态 —— 点行名会顺手收起刚展开的分支',
    );
  });

  it('展开 / 收起由一个单独的函数负责', () => {
    const src = code(readSource(SELECTION));
    assert.match(
      bodyOf(src, 'toggleExpanded'),
      /toggleNodeExpanded\(id\)/,
      '展开箭头没有接到切换展开状态的那一个动作上',
    );
    assert.match(
      src,
      /return \{[\s\S]*?toggleExpanded,/,
      '展开 / 收起那个函数没有对外提供 —— 展开箭头接不到它',
    );
  });
});

describe('展开箭头：它自己是另一个按钮', () => {
  it('箭头按钮只调展开那一个动作', () => {
    const arrow = elementAround(code(readSource(SHELL)), 'data-testid="tree-toggle-btn"');
    assert.match(
      arrow,
      /onClick=\{toggle\.onToggle\}/,
      '箭头按钮没有只调展开那一个动作 —— 点箭头会顺手打开工作区',
    );
    assert.match(arrow, /aria-expanded=\{toggle\.expanded\}/, '箭头按钮没有报告当前的展开状态');
  });

  it('箭头按钮不是另一个按钮里套着的按钮', () => {
    const arrow = elementAround(code(readSource(SHELL)), 'data-testid="tree-toggle-btn"');
    const opens = arrow.match(/<button/g)?.length ?? 0;
    const closes = arrow.match(/<\/button>/g)?.length ?? 0;
    assert.equal(opens, 1, '箭头按钮里还套着别的按钮 —— 浏览器会把两个点击合成一个');
    assert.equal(closes, 1, '箭头按钮的标签没有配平');
  });

  it('箭头按钮在行按钮外面，不是行按钮的一部分', () => {
    const src = code(readSource(SHELL));
    const arrowAt = src.indexOf('data-testid="tree-toggle-btn"');
    const labelAt = src.indexOf('onClick={onSelect}');
    assert.ok(arrowAt !== -1 && labelAt !== -1);
    assert.ok(
      arrowAt < labelAt,
      '箭头按钮排在行按钮之后 —— 它被渲染在行按钮里面，点它必然同时触发打开工作区',
    );
  });

  it('行按钮不碰展开状态', () => {
    const label = elementAround(code(readSource(SHELL)), 'onClick={onSelect}');
    // 断言的是「没有调展开那一个动作」，故只查动作本身 —— 行按钮的 class 里出现
    // `toggle` 这个词（`toggle ? … : …` 决定左边距）是另一回事。
    assert.doesNotMatch(
      label,
      /onToggle|toggleNodeExpanded/,
      '行按钮里出现了展开那一个动作 —— 点行名会顺手收起刚展开的分支',
    );
  });
});

describe('箭头不发起拖拽，但仍然是放置目标', () => {
  it('箭头不发起拖拽', () => {
    const arrow = elementAround(code(readSource(SHELL)), 'data-testid="tree-toggle-btn"');
    assert.doesNotMatch(arrow, /draggable/, '箭头又能拖了 —— 拖箭头会让人以为这一行能被拖走');
    assert.doesNotMatch(
      arrow,
      /onDragStart/,
      '箭头又能发起拖拽了 —— 拖箭头会让人以为这一行能被拖走',
    );
  });

  it('箭头仍然是这一行的放置目标', () => {
    const arrow = elementAround(code(readSource(SHELL)), 'data-testid="tree-toggle-btn"');
    assert.match(arrow, /onDrop=/, '箭头不接放置了 —— 把节点拖到箭头那一小条上会落空');
  });
});

describe('模块行与集合行：都接上了箭头', () => {
  it('模块行把展开状态与动作交给共享行外壳', () => {
    assert.match(
      code(readSource('../rows/ModuleRow.tsx')),
      /toggle=\{\{/,
      '模块行没有展开箭头 —— 模块的子级没法单独展开',
    );
  });

  it('集合行把展开状态与动作交给共享行外壳', () => {
    assert.match(
      code(readSource('../rows/CollectionRow.tsx')),
      /toggle=\{\{/,
      '集合行没有展开箭头 —— 集合的子级没法单独展开',
    );
  });

  it('集合分支把动作转给每一个集合', () => {
    assert.match(
      code(readSource('../rows/CollectionBranch.tsx')),
      /onToggle=\{\(\) => onToggleCollection\(collection\.id\)\}/,
      '集合分支没有把展开动作转给集合行 —— 集合的箭头点了没有反应',
    );
  });

  it('树把展开动作接到共享行外壳上', () => {
    const src = code(readSource('../ProjectTree.tsx'));
    assert.match(
      src,
      /onToggle=\{\(\) => toggleExpanded\(modBranch\.module\.id\)\}/,
      '模块行的箭头没有接到展开那一个动作上',
    );
    assert.match(
      src,
      /onToggleCollection=\{toggleExpanded\}/,
      '集合行的箭头没有接到展开那一个动作上',
    );
  });
});
