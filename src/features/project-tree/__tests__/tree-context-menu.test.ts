/**
 * 「右键一个接口不激活它」的接线回归网 —— 读源码，不渲染 React。
 *
 * **为什么只能读源码。** 本 runner（`tsx --test` + `node:assert`）没有 DOM、没有
 * jsdom、没有 React 渲染器，故测不了树的实际交互。读源码是钝办法，但这是本仓库对
 * 「功能测试看不见的那类缺陷」的既定取舍，见
 * `src/features/response-viewer/__tests__/response-viewer-layout.test.ts`，理由相同。
 *
 * **它钉住的是什么。** 树里**左键**点接口 = 「我要在它上面干活」，故它要开标签并跳转；
 * 树里**右键**点接口 = 「看一眼菜单」，故它只弹菜单，不调用 `setActiveEndpoint`。
 *
 * 行为层面靠真机验证；这里守的是**接线**：那两处右键入口一旦又接回激活动作，右键就会
 * 重新抢走活动标签。
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
 * 必须剥：注释里会提到 `setActiveEndpoint` 这样的**反例**，不剥的话源码级断言会被
 * 自己的说明文字满足 —— 一条不可能失败的回归测试比没有更糟。
 */
function code(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
}

const EDIT_MENU = code(readSource('../hooks/useEditMenu.tsx'));
const PROJECT_TREE = code(readSource('../ProjectTree.tsx'));
const TREE_SELECTION = code(readSource('../hooks/useTreeSelection.ts'));

/** 右键处理器的函数体。 */
function openEndpointMenuBody(): string {
  const m = EDIT_MENU.match(/const openEndpointMenu = [\s\S]*?\n {2}\};/);
  assert.ok(m, 'useEditMenu 里没有 openEndpointMenu —— 右键入口改道了');
  return m[0];
}

/** 共用的右键菜单处理器的函数体。 */
function openMenuBody(): string {
  const m = EDIT_MENU.match(/const openMenu = [\s\S]*?\n {2}\};/);
  assert.ok(m, 'useEditMenu 里没有 openMenu');
  return m[0];
}

/** 左键点击处理器的函数体。 */
function handleEndpointClickBody(): string {
  const m = TREE_SELECTION.match(/const handleEndpointClick = [\s\S]*?\n {2}\};/);
  assert.ok(m, 'useTreeSelection 里没有 handleEndpointClick');
  return m[0];
}

/** 左键开接口标签的处理器的函数体。 */
function openEndpointTabBody(): string {
  const m = TREE_SELECTION.match(/const openEndpointTab = [\s\S]*?\n {2}\};/);
  assert.ok(m, 'useTreeSelection 里没有 openEndpointTab');
  return m[0];
}

describe('右键一个接口：只弹菜单，不激活它', () => {
  it('右键处理器里没有激活接口的动作', () => {
    const body = openEndpointMenuBody();
    assert.equal(
      /\bsetActiveEndpoint\s*\(/.test(body),
      false,
      '右键又去激活接口了：右键只是要看一眼菜单，不该把那个接口的标签打开并跳过去',
    );
    assert.equal(
      /\bsetActiveEndpoint\s*\(/.test(openMenuBody()),
      false,
      '右键共用的 openMenu 又去激活接口了：它也服务模块与集合，接口右键不该抢活动标签',
    );
    assert.equal(
      /\bopenEndpointTab\s*\(/.test(body),
      false,
      '右键又去开标签了：标签只该由「我要在它上面干活」的那些动作开',
    );
    assert.equal(
      /\bopenTab\s*\(/.test(body),
      false,
      '右键又去开标签了：标签只该由「我要在它上面干活」的那些动作开',
    );
  });

  it('右键处理器会真正弹出菜单', () => {
    assert.match(
      openEndpointMenuBody(),
      /\bopenMenu\s*\(\s*e\s*,\s*'endpoint'\s*,\s*id\s*\)/,
      '右键处理器没有调用 openMenu：右键不再弹出菜单',
    );
  });

  it('树里的两处右键入口都走那一个处理器', () => {
    const wired = [...PROJECT_TREE.matchAll(/onEndpointContextMenu=\{([^}]*)\}/g)].map((m) =>
      m[1].trim(),
    );
    assert.equal(
      wired.length,
      2,
      '右键入口的数目变了 —— 模块下的接口与集合里的接口各一处，少一处就有一处仍会抢活动标签',
    );
    for (const target of wired) {
      assert.equal(
        target,
        'openEndpointMenu',
        `右键入口接到了 ${target} 上，而不是那个不激活接口的处理器`,
      );
    }
  });
});

describe('左键一个接口：仍然要激活它', () => {
  it('左键路径仍然会激活接口', () => {
    assert.match(
      handleEndpointClickBody(),
      /\bopenEndpointTab\s*\(\s*id\s*\)/,
      '左键点接口不再开标签了：单击一行本该打开它的标签并跳过去',
    );
    assert.match(
      openEndpointTabBody(),
      /\bsetActiveEndpoint\s*\(\s*id\s*\)/,
      '开接口标签不再激活接口了：标签打开后树不会跳到该接口',
    );
  });
});
