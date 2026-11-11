/**
 * 「点箭头只展开、点行名只打开工作区」的接线回归网 —— 读源码，不渲染 React。
 *
 * **为什么只能读源码。** 本 runner（`tsx --test` + `node:assert`）没有 DOM、没有
 * jsdom、没有 React 渲染器，故测不了真实的鼠标点击。读源码是钝办法，但这是本仓库对
 * 「功能测试看不见的那类缺陷」的既定取舍，见 `tree-rename-selection.test.ts` 的开头，
 * 理由相同。
 *
 * **它钉住的是什么。** 「打开工作区」与「展开 / 收起」必须各自独立：行按钮调
 * `openTab`，展开箭头调 `toggleNodeExpanded`，两个按钮是两个元素。
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
 * 必须剥：注释里会写出与断言相同的字面量（例如提到 `onDrop=`），不剥的话源码级断言
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

  it('箭头按钮不在行按钮里，也没有被存成变量再插进行按钮', () => {
    const shell = code(readSource(SHELL));
    const label = elementAround(shell, 'onClick={onSelect}');
    assert.doesNotMatch(
      label,
      /tree-toggle-btn/,
      '箭头按钮的标记出现在行按钮里 —— 它被渲染在行按钮内部',
    );
    // 箭头按钮若被存成变量再插进行按钮，上面那条断言看不见它 —— 源码级的判据到此为止，
    // 仓库没有渲染器可用来断言真正的元素嵌套（见本文件开头）。
    assert.doesNotMatch(
      shell,
      /=\s*\(?\s*<button/,
      '箭头按钮被存成了变量 —— 源码级断言看不见它最终被插在哪个按钮里',
    );
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

describe('箭头与名称：两块各自独立的方块', () => {
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

  it('箭头有自己的圆角边框与阴影', () => {
    const arrow = elementAround(code(readSource(SHELL)), 'data-testid="tree-toggle-btn"');
    assert.match(arrow, /rounded-/, '箭头没有圆角');
    assert.match(arrow, /border/, '箭头没有边框');
    assert.match(arrow, /shadow/, '箭头没有阴影');
  });

  it('名称那一块也有自己的圆角边框与阴影', () => {
    const label = elementAround(code(readSource(SHELL)), 'onClick={onSelect}');
    assert.match(label, /rounded-/, '名称那一块没有圆角');
    assert.match(label, /border/, '名称那一块没有边框');
    assert.match(label, /shadow/, '名称那一块没有阴影');
  });

  it('名称左侧对齐，不居中', () => {
    const label = elementAround(code(readSource(SHELL)), 'onClick={onSelect}');
    assert.match(label, /text-left/, '名称那一块不是左对齐');
  });

  it('名称那一块的边框与阴影从左向右渐变消失', () => {
    const shell = code(readSource(SHELL));
    assert.match(shell, /linear-gradient\(to right/, '名称那一块的边框没有从左向右的渐变遮罩');
    assert.match(shell, /mask(Image|-image)/, '渐变没有落在遮罩上 —— 文字会跟着一起淡掉');
    const label = elementAround(shell, 'onClick={onSelect}');
    assert.match(label, /FADE_BORDER_STYLE/, '名称那一块没有用那一份渐变遮罩');
    assert.match(
      label,
      /pointer-events-none/,
      '渐变那一层没有关掉指针事件 —— 它会挡住这一行的点击',
    );
  });

  it('箭头与名称两块之间只隔一点', () => {
    const shell = code(readSource(SHELL));
    assert.match(
      shell,
      /items-center gap-0\.5/,
      '行容器不再用 2px 的间隙 —— 箭头与名称两块之间隔得太远',
    );
    const label = elementAround(shell, 'onClick={onSelect}');
    assert.match(
      label,
      /\bpx-1\.5\b/,
      '名称那一块的左右内边距不是 6px —— 名称离自己那一块的边框太远',
    );
  });

  it('箭头与名称两块同高', () => {
    const shell = code(readSource(SHELL));
    const blockH = shell.match(/const BLOCK_H = '([^']+)'/)?.[1];
    assert.ok(blockH, '共享行外壳里没有写死块高度的那一个常量');
    const arrow = elementAround(shell, 'data-testid="tree-toggle-btn"');
    const label = elementAround(shell, 'onClick={onSelect}');
    assert.match(arrow, /BLOCK_H/, '箭头方块没有用统一的高度');
    assert.match(label, /BLOCK_H/, '名称方块没有用统一的高度');
  });

  it('药丸留在名称方块里面', () => {
    const label = elementAround(code(readSource(SHELL)), 'onClick={onSelect}');
    assert.match(label, /\{leading\}/, '药丸被搬出了名称方块 —— 药丸与名称不再是一体的');
  });

  it('接口行左侧留出一格，名称方块才与上级对齐', () => {
    const shell = code(readSource(SHELL));
    assert.match(
      shell,
      /!toggle && leading/,
      '接口行没有为对齐留出左侧那一格 —— 它的名称方块落在上级名称方块的左边',
    );
    assert.match(shell, /<span aria-hidden[^>]*SLOT_W/, '那一格不是与箭头同样宽的空格');
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
